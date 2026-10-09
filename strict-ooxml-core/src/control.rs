//! Progress and cancellation of a long open or render.
//!
//! A large document takes seconds to open and render, and the call that does it
//! blocks. [`OpenControl`] is the handle a caller keeps to watch that call from
//! another thread and to stop it:
//!
//! ```
//! use std::time::Duration;
//! use strict_ooxml_core::control::OpenControl;
//!
//! let control = OpenControl::with_timeout(Duration::from_secs(30));
//! // pass `control.clone()` in `OpenOptions::control`, then from a UI thread:
//! let progress = control.progress();
//! println!("{:?}: {} of {}", progress.stage, progress.done, progress.total);
//! control.cancel(); // the open returns `StrictError::Cancelled`
//! ```
//!
//! # Model
//!
//! - **Polled, not called back.** The work updates a few atomics; the caller reads
//!   them when it likes. No caller code runs in the middle of a parse, so there is
//!   nothing to re-enter and nothing that can block the work.
//! - **Checked at checkpoints.** The XML reader, the ZIP inflater, the
//!   normalizer and the layout loops call [`checkpoint`], which reads one atomic
//!   and, every few thousand calls, the clock. Cancellation is seen within
//!   milliseconds, and a run with no control installed pays one thread-local
//!   read per checkpoint.
//! - **Scoped to the thread that does the work.** An entry point that takes a
//!   control (`Package::open_*`, the facade's `StrictDocument::open_*`, the
//!   renderer) installs it with [`OpenControl::enter`] for the duration of the
//!   call; the checkpoints find it there. A caller driving lower-level functions
//!   itself (`parse_document`, say) does the same:
//!
//! ```
//! # use strict_ooxml_core::control::OpenControl;
//! let control = OpenControl::new();
//! let _scope = control.enter();
//! // ... any parse or render on this thread now honours `control` ...
//! ```

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::error::{Result, StrictError};
use crate::part::PartId;

/// Why a run stopped early.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CancelReason {
    /// [`OpenControl::cancel`] was called.
    Requested,
    /// The deadline given to [`OpenControl::with_deadline`] passed.
    Deadline,
}

impl std::fmt::Display for CancelReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Requested => "cancelled by the caller",
            Self::Deadline => "deadline passed",
        })
    }
}

/// What a run is doing now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Stage {
    /// Nothing started yet.
    #[default]
    Idle,
    /// Reading the archive into memory; `done`/`total` are bytes (`total` is
    /// 0 when the size is not known in advance).
    ReadingInput,
    /// Checking the ZIP directory, content types and relationships.
    OpeningPackage,
    /// Inflating the main document part; bytes of uncompressed output.
    InflatingDocument,
    /// Normalizing the main document part; bytes of input consumed.
    NormalizingDocument,
    /// Parsing the main document part; bytes of input consumed.
    ParsingDocument,
    /// Parsing headers, footers, notes, styles, numbering and settings.
    ParsingParts,
    /// Laying out pages; `done` is pages laid out, `total` is 0.
    Layout,
    /// Writing pages out; `done`/`total` are pages.
    Painting,
    /// The run finished.
    Finished,
}

impl Stage {
    const ALL: [Self; 10] = [
        Self::Idle,
        Self::ReadingInput,
        Self::OpeningPackage,
        Self::InflatingDocument,
        Self::NormalizingDocument,
        Self::ParsingDocument,
        Self::ParsingParts,
        Self::Layout,
        Self::Painting,
        Self::Finished,
    ];

    fn code(self) -> u8 {
        Self::ALL
            .iter()
            .position(|stage| *stage == self)
            .and_then(|index| u8::try_from(index).ok())
            .unwrap_or(0)
    }

    fn from_code(code: u8) -> Self {
        Self::ALL
            .get(usize::from(code))
            .copied()
            .unwrap_or_default()
    }
}

/// A snapshot of a run's progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Progress {
    /// The stage the run is in.
    pub stage: Stage,
    /// Units of the stage done; see [`Stage`] for the unit.
    pub done: u64,
    /// Units of the stage in all, or 0 when not known.
    pub total: u64,
}

#[derive(Debug)]
struct Shared {
    cancelled: AtomicBool,
    deadline: Option<Instant>,
    stage: AtomicU8,
    done: AtomicU64,
    total: AtomicU64,
    /// The part whose bytes the `*Document` stages count: the main document.
    tracked: Mutex<Option<PartId>>,
}

/// A handle to watch and stop a long open or render. Cheap to clone; every
/// clone is the same control.
#[derive(Clone, Debug)]
pub struct OpenControl {
    shared: Arc<Shared>,
}

impl Default for OpenControl {
    fn default() -> Self {
        Self::new()
    }
}

impl OpenControl {
    /// A control with no deadline.
    #[must_use]
    pub fn new() -> Self {
        Self::build(None)
    }

    /// A control that cancels the run once `deadline` passes.
    #[must_use]
    pub fn with_deadline(deadline: Instant) -> Self {
        Self::build(Some(deadline))
    }

    /// A control that cancels the run `timeout` from now.
    #[must_use]
    pub fn with_timeout(timeout: Duration) -> Self {
        Self::build(Instant::now().checked_add(timeout))
    }

    fn build(deadline: Option<Instant>) -> Self {
        Self {
            shared: Arc::new(Shared {
                cancelled: AtomicBool::new(false),
                deadline,
                stage: AtomicU8::new(Stage::Idle.code()),
                done: AtomicU64::new(0),
                total: AtomicU64::new(0),
                tracked: Mutex::new(None),
            }),
        }
    }

    /// Asks the run to stop. It returns [`StrictError::Cancelled`] from its next
    /// checkpoint; a run that already finished is not affected.
    pub fn cancel(&self) {
        self.shared.cancelled.store(true, Ordering::Relaxed);
    }

    /// Whether [`cancel`](Self::cancel) was called or the deadline passed.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.reason().is_some()
    }

    /// The current progress.
    #[must_use]
    pub fn progress(&self) -> Progress {
        Progress {
            stage: Stage::from_code(self.shared.stage.load(Ordering::Relaxed)),
            done: self.shared.done.load(Ordering::Relaxed),
            total: self.shared.total.load(Ordering::Relaxed),
        }
    }

    /// Installs this control for the current thread until the returned scope is
    /// dropped. Scopes nest: dropping one restores whatever was installed before.
    #[must_use = "the control is installed only while the scope is alive"]
    pub fn enter(&self) -> ControlScope {
        let previous = CURRENT.with(|current| current.replace(Some(self.clone())));
        ControlScope { previous }
    }

    fn reason(&self) -> Option<CancelReason> {
        if self.shared.cancelled.load(Ordering::Relaxed) {
            return Some(CancelReason::Requested);
        }
        match self.shared.deadline {
            Some(deadline) if Instant::now() >= deadline => Some(CancelReason::Deadline),
            _ => None,
        }
    }

    fn set_stage(&self, stage: Stage, total: u64) {
        self.shared.done.store(0, Ordering::Relaxed);
        self.shared.total.store(total, Ordering::Relaxed);
        self.shared.stage.store(stage.code(), Ordering::Relaxed);
    }
}

/// Keeps an [`OpenControl`] installed on the current thread; see
/// [`OpenControl::enter`].
#[derive(Debug)]
pub struct ControlScope {
    previous: Option<OpenControl>,
}

impl Drop for ControlScope {
    fn drop(&mut self) {
        let previous = self.previous.take();
        CURRENT.with(|current| *current.borrow_mut() = previous);
    }
}

thread_local! {
    static CURRENT: RefCell<Option<OpenControl>> = const { RefCell::new(None) };
    /// Checkpoints since the clock was last read on this thread.
    static TICKS: Cell<u32> = const { Cell::new(0) };
}

/// How many checkpoints pass between two reads of the clock.
const CLOCK_EVERY: u32 = 1024;

/// The control installed on this thread, if any.
#[must_use]
pub fn current() -> Option<OpenControl> {
    CURRENT.with(|current| current.borrow().clone())
}

/// Returns [`StrictError::Cancelled`] when the installed control was cancelled
/// or its deadline passed; `Ok` when there is no control.
///
/// Cheap enough for an inner loop: the cancel flag is one atomic load, and the
/// clock is read once every `CLOCK_EVERY` (1024) calls.
///
/// # Errors
///
/// [`StrictError::Cancelled`] as above.
pub fn checkpoint() -> Result<()> {
    CURRENT.with(|current| {
        let current = current.borrow();
        let Some(control) = current.as_ref() else {
            return Ok(());
        };
        if control.shared.cancelled.load(Ordering::Relaxed) {
            return Err(StrictError::Cancelled {
                reason: CancelReason::Requested,
            });
        }
        if control.shared.deadline.is_none() {
            return Ok(());
        }
        let due = TICKS.with(|ticks| {
            let next = ticks.get().wrapping_add(1);
            ticks.set(next % CLOCK_EVERY);
            next >= CLOCK_EVERY
        });
        if !due {
            return Ok(());
        }
        match control.reason() {
            Some(reason) => Err(StrictError::Cancelled { reason }),
            None => Ok(()),
        }
    })
}

/// Like [`checkpoint`], but always reads the clock: for the coarse places (a
/// part, a page) where a call is rare and a missed deadline would be noticed.
///
/// # Errors
///
/// [`StrictError::Cancelled`] when cancelled or past the deadline.
pub fn checkpoint_now() -> Result<()> {
    CURRENT.with(
        |current| match current.borrow().as_ref().and_then(OpenControl::reason) {
            Some(reason) => Err(StrictError::Cancelled { reason }),
            None => Ok(()),
        },
    )
}

/// Enters `stage` with `total` units, resetting `done`. No-op without a control.
pub fn stage(stage: Stage, total: u64) {
    CURRENT.with(|current| {
        if let Some(control) = current.borrow().as_ref() {
            control.set_stage(stage, total);
        }
    });
}

/// Records `done` units of the current stage. No-op without a control.
pub fn advance(done: u64) {
    CURRENT.with(|current| {
        if let Some(control) = current.borrow().as_ref() {
            control.shared.done.store(done, Ordering::Relaxed);
        }
    });
}

/// Names the part whose reading, inflating, normalizing and parsing the
/// `*Document` stages measure. No-op without a control.
///
/// Only that part moves the progress bar: a chart or a header read while the
/// main document is being parsed would otherwise make it jump back.
pub fn track(part: &PartId) {
    CURRENT.with(|current| {
        if let Some(control) = current.borrow().as_ref() {
            if let Ok(mut tracked) = control.shared.tracked.lock() {
                *tracked = Some(part.clone());
            }
        }
    });
}

/// Whether `part` is the one [`track`] named, under an installed control.
#[must_use]
pub fn is_tracked(part: &PartId) -> bool {
    CURRENT.with(|current| {
        current.borrow().as_ref().is_some_and(|control| {
            control
                .shared
                .tracked
                .lock()
                .is_ok_and(|tracked| tracked.as_ref() == Some(part))
        })
    })
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{
        advance, checkpoint, checkpoint_now, current, is_tracked, stage, track, CancelReason,
        OpenControl, Stage,
    };
    use crate::error::StrictError;
    use crate::part::PartId;

    #[test]
    fn without_a_control_every_checkpoint_passes() {
        assert!(current().is_none());
        assert!(checkpoint().is_ok());
        assert!(checkpoint_now().is_ok());
        stage(Stage::Layout, 3);
        advance(1);
    }

    #[test]
    fn a_cancelled_control_stops_the_next_checkpoint() {
        let control = OpenControl::new();
        let _scope = control.enter();
        assert!(checkpoint().is_ok());
        control.clone().cancel();
        assert!(matches!(
            checkpoint(),
            Err(StrictError::Cancelled {
                reason: CancelReason::Requested
            })
        ));
    }

    #[test]
    fn a_passed_deadline_is_seen_within_the_clock_interval() {
        let control = OpenControl::with_deadline(Instant::now());
        let _scope = control.enter();
        assert!(matches!(
            checkpoint_now(),
            Err(StrictError::Cancelled {
                reason: CancelReason::Deadline
            })
        ));
        let stopped = (0..=super::CLOCK_EVERY).any(|_| checkpoint().is_err());
        assert!(stopped, "the clock is read at least once per interval");
    }

    #[test]
    fn a_distant_deadline_does_not_stop_the_run() {
        let control = OpenControl::with_timeout(Duration::from_secs(3600));
        let _scope = control.enter();
        assert!((0..5000).all(|_| checkpoint().is_ok()));
        assert!(!control.is_cancelled());
    }

    #[test]
    fn progress_is_reported_through_the_installed_control() {
        let control = OpenControl::new();
        {
            let _scope = control.enter();
            stage(Stage::ParsingDocument, 100);
            advance(40);
        }
        let progress = control.progress();
        assert_eq!(progress.stage, Stage::ParsingDocument);
        assert_eq!((progress.done, progress.total), (40, 100));
        assert!(current().is_none(), "the scope ended with its guard");
    }

    #[test]
    fn scopes_nest_and_restore() {
        let outer = OpenControl::new();
        let inner = OpenControl::new();
        let _outer = outer.enter();
        {
            let _inner = inner.enter();
            stage(Stage::Layout, 0);
        }
        stage(Stage::Painting, 2);
        assert_eq!(inner.progress().stage, Stage::Layout);
        assert_eq!(outer.progress().stage, Stage::Painting);
    }

    #[test]
    fn only_the_tracked_part_is_tracked() {
        let main = PartId::new("/word/document.xml");
        assert!(!is_tracked(&main), "nothing is tracked without a control");
        let control = OpenControl::new();
        let _scope = control.enter();
        assert!(!is_tracked(&main));
        track(&main);
        assert!(is_tracked(&main));
        assert!(!is_tracked(&PartId::new("/word/header1.xml")));
    }

    #[test]
    fn every_stage_round_trips_through_its_code() {
        for stage in Stage::ALL {
            assert_eq!(Stage::from_code(stage.code()), stage);
        }
    }
}
