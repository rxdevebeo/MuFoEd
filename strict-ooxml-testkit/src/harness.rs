//! Running a closure the way a hostile input would meet it in production.
//!
//! The closure runs on its own thread with a 1 MiB stack — the size of the main
//! thread on Windows, and smaller than the 2 MiB test threads get — and the
//! caller waits at most ten seconds for it.
//!
//! A stack overflow is not a panic: it aborts the whole test process. That is
//! still a failing test, just a louder one, and the small stack is what makes it
//! happen in CI at the depth where it would happen for a user.

use std::panic::{self, AssertUnwindSafe};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// Stack size of [`bounded`].
pub const STACK: usize = 1 << 20;

/// Time limit of [`bounded`].
pub const TIMEOUT: Duration = Duration::from_secs(10);

/// How a bounded run ended.
#[derive(Debug)]
pub enum Outcome<T> {
    /// The closure returned.
    Returned(T),
    /// The closure panicked; the payload, if it was a string.
    Panicked(String),
    /// The closure did not return within the limit. Its thread is left running.
    TimedOut,
}

/// Runs `f` on a [`STACK`]-sized thread and waits at most [`TIMEOUT`].
pub fn bounded<T, F>(f: F) -> Outcome<T>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    bounded_with(STACK, TIMEOUT, f)
}

/// Runs `f` on a thread of `stack` bytes and waits at most `timeout`.
///
/// # Panics
///
/// If the thread cannot be spawned.
pub fn bounded_with<T, F>(stack: usize, timeout: Duration, f: F) -> Outcome<T>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let (sender, receiver) = mpsc::channel();
    thread::Builder::new()
        .name("testkit-bounded".to_owned())
        .stack_size(stack)
        .spawn(move || {
            let result = panic::catch_unwind(AssertUnwindSafe(f));
            let _ = sender.send(result);
        })
        .expect("spawn the bounded thread");
    match receiver.recv_timeout(timeout) {
        Ok(Ok(value)) => Outcome::Returned(value),
        Ok(Err(payload)) => Outcome::Panicked(panic_message(payload.as_ref())),
        Err(_) => Outcome::TimedOut,
    }
}

/// Runs `f` with [`bounded`] and returns its value; panics with a message
/// naming `what` if it panicked or hung.
pub fn assert_survives<T, F>(what: &str, f: F) -> T
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    match bounded(f) {
        Outcome::Returned(value) => value,
        Outcome::Panicked(message) => panic!("{what}: panicked: {message}"),
        Outcome::TimedOut => panic!("{what}: did not return within {TIMEOUT:?}"),
    }
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "<non-string panic payload>".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_returned_value_comes_back() {
        assert!(matches!(bounded(|| 7), Outcome::Returned(7)));
    }

    #[test]
    fn a_panic_is_caught_with_its_message() {
        let outcome = bounded(|| -> () { panic!("boom") });
        assert!(matches!(outcome, Outcome::Panicked(ref m) if m == "boom"));
    }

    #[test]
    fn a_hang_times_out() {
        let outcome = bounded_with(STACK, Duration::from_millis(50), || {
            thread::sleep(Duration::from_secs(2));
        });
        assert!(matches!(outcome, Outcome::TimedOut));
    }
}
