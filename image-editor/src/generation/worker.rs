//! One process-lifetime inference lane. Model residency remains owned by Runtime leases.
//! Windows cuDNN cannot safely destroy its thread-local handles during thread teardown.
//! A stable lane also reuses CUDA context/kernel state instead of rebuilding it per pass.
use std::{
    io,
    sync::{mpsc, OnceLock},
};
type Work = Box<dyn FnOnce() + Send + 'static>;
pub(super) fn dispatch(work: Work) -> io::Result<()> {
    static WORKER: OnceLock<Result<mpsc::SyncSender<Work>, String>> = OnceLock::new();
    let sender = WORKER
        .get_or_init(|| {
            let (sender, receiver) = mpsc::sync_channel::<Work>(1);
            std::thread::Builder::new()
                .name("image-studio-inference".into())
                .spawn(move || {
                    for work in receiver {
                        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).is_err() {
                            eprintln!("[inference] task panicked; lane remains available");
                        }
                    }
                })
                .map_err(|e| e.to_string())?;
            Ok(sender)
        })
        .as_ref()
        .map_err(|error| io::Error::other(error.clone()))?;
    sender
        .try_send(work)
        .map_err(|e| io::Error::other(e.to_string()))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lane_reuses_thread_and_survives_task_unwind() {
        let (sender, receiver) = mpsc::channel();
        let first = sender.clone();
        dispatch(Box::new(move || {
            first.send(std::thread::current().id()).unwrap();
        }))
        .unwrap();
        let first = receiver
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        dispatch(Box::new(|| panic!("expected diagnostic task panic"))).unwrap();
        // The bounded queue may still contain the preceding task until it is consumed.
        let start = std::time::Instant::now();
        loop {
            let sender = sender.clone();
            if dispatch(Box::new(move || {
                sender.send(std::thread::current().id()).unwrap();
            }))
            .is_ok()
            {
                break;
            }
            assert!(start.elapsed() < std::time::Duration::from_secs(2));
            std::thread::yield_now();
        }
        assert_eq!(
            first,
            receiver
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap()
        );
    }
}
