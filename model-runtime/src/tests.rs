use super::*;

fn task(owner: &str) -> Task {
    Task::new(owner, "test-backend")
}

#[test]
fn text_vision_and_images_share_fifo_admission() {
    let runtime = Runtime::new(Policy::default());
    let mut text = runtime.enqueue(task("text")).unwrap();
    let mut image = runtime.clone().enqueue(task("image")).unwrap();
    let mut vision = runtime.enqueue(task("vision")).unwrap();
    assert!(image.try_start().unwrap().is_none());
    let text_lease = text.try_start().unwrap().unwrap();
    assert_eq!(runtime.snapshot().active.unwrap().owner, "text");
    assert!(image.try_start().unwrap().is_none());
    drop(text_lease);
    assert!(vision.try_start().unwrap().is_none());
    let image_lease = image.try_start().unwrap().unwrap();
    drop(image_lease);
    drop(vision.try_start().unwrap().unwrap());
    assert_eq!(runtime.snapshot(), Snapshot::default());
}

#[test]
fn cancelling_a_waiter_preserves_other_entries() {
    let runtime = Runtime::new(Policy::default());
    let first = runtime.enqueue(task("text")).unwrap();
    let mut next = runtime.enqueue(task("image")).unwrap();
    drop(first);
    let lease = next.try_start().unwrap().unwrap();
    assert!(matches!(next.try_start(), Err(Error::AlreadyStarted)));
    drop(next);
    assert!(runtime.snapshot().active.is_some());
    drop(lease);
    assert!(runtime.snapshot().active.is_none());
}

#[test]
fn cancelled_ui_handle_does_not_release_a_live_worker() {
    let runtime = Runtime::new(Policy::default());
    let cancelled = Arc::new(AtomicBool::new(false));
    let ticket = runtime.enqueue(task("text")).unwrap();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (finish_tx, finish_rx) = std::sync::mpsc::channel();
    let flag = cancelled.clone();
    let worker = std::thread::spawn(move || {
        let _lease = ticket.wait(&flag, Duration::from_secs(5)).unwrap();
        started_tx.send(()).unwrap();
        finish_rx.recv().unwrap();
    });
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    cancelled.store(true, Ordering::Release);
    let mut image = runtime.enqueue(task("image")).unwrap();
    assert!(image.try_start().unwrap().is_none());
    finish_tx.send(()).unwrap();
    worker.join().unwrap();
    assert!(image.try_start().unwrap().is_some());
}

#[test]
fn queue_wait_cancel_and_deadline_remove_the_entry() {
    let runtime = Runtime::new(Policy::default());
    let cancelled = AtomicBool::new(true);
    assert!(matches!(
        runtime
            .enqueue(task("text"))
            .unwrap()
            .wait(&cancelled, Duration::from_secs(1)),
        Err(Error::Cancelled)
    ));
    assert!(matches!(
        runtime
            .enqueue(task("text"))
            .unwrap()
            .wait(&AtomicBool::new(false), Duration::ZERO),
        Err(Error::Timeout)
    ));
    assert!(runtime.snapshot().queued.is_empty());
}

#[test]
fn strict_budgets_reject_unknown_and_oversized_peaks_before_queueing() {
    let runtime = Runtime::new(Policy {
        ram: Some(100),
        vram: Some(50),
        ..Policy::default()
    });
    assert!(matches!(
        runtime.enqueue(task("image")),
        Err(Error::UnknownEstimate("RAM"))
    ));
    let mut request = task("image");
    request.memory = Memory {
        ram: Some(101),
        vram: Some(1),
    };
    assert!(matches!(
        runtime.enqueue(request.clone()),
        Err(Error::OverBudget {
            resource: "RAM",
            ..
        })
    ));
    request.memory = Memory {
        ram: Some(100),
        vram: Some(51),
    };
    assert!(matches!(
        runtime.enqueue(request.clone()),
        Err(Error::OverBudget {
            resource: "VRAM",
            ..
        })
    ));
    request.memory.vram = Some(50);
    drop(runtime.enqueue(request).unwrap());
    assert_eq!(runtime.snapshot(), Snapshot::default());
}

#[test]
fn full_queue_does_not_discard_an_existing_job() {
    let runtime = Runtime::new(Policy {
        queue_capacity: 1,
        ..Policy::default()
    });
    let mut ticket = runtime.enqueue(task("text")).unwrap();
    assert!(matches!(
        runtime.enqueue(task("image")),
        Err(Error::QueueFull)
    ));
    let lease = ticket.try_start().unwrap().unwrap();
    assert!(runtime.enqueue(task("image")).is_ok());
    drop(lease);
}

#[test]
fn backend_panic_releases_admission_during_unwind() {
    let runtime = Runtime::new(Policy::default());
    let mut ticket = runtime.enqueue(task("text")).unwrap();
    let lease = ticket.try_start().unwrap().unwrap();
    assert!(std::thread::spawn(move || {
        let _lease = lease;
        panic!("simulated backend failure");
    })
    .join()
    .is_err());
    assert_eq!(runtime.snapshot(), Snapshot::default());
}

#[test]
fn concurrent_backends_never_overlap() {
    let runtime = Runtime::new(Policy::default());
    let active = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let workers: Vec<_> = (0..16)
        .map(|index| {
            let ticket = runtime
                .enqueue(task(if index % 2 == 0 { "text" } else { "image" }))
                .unwrap();
            let active = active.clone();
            std::thread::spawn(move || {
                let _lease = ticket
                    .wait(&AtomicBool::new(false), Duration::from_secs(5))
                    .unwrap();
                assert_eq!(active.fetch_add(1, Ordering::AcqRel), 0);
                std::thread::yield_now();
                assert_eq!(active.fetch_sub(1, Ordering::AcqRel), 1);
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    assert_eq!(runtime.snapshot(), Snapshot::default());
}

#[test]
fn shutdown_rejects_new_producers_and_waits_for_active_lease() {
    let runtime = Runtime::new(Policy {
        queue_capacity: 1,
        ..Policy::default()
    });
    let lease = runtime
        .enqueue(task("active"))
        .unwrap()
        .try_start()
        .unwrap()
        .unwrap();
    let mut waiting = runtime.enqueue(task("queued")).unwrap();
    let released = runtime.shutdown_async();
    assert!(matches!(
        runtime.enqueue(task("new")),
        Err(Error::ShuttingDown)
    ));
    assert!(matches!(waiting.try_start(), Err(Error::ShuttingDown)));
    drop(waiting);
    assert!(released.try_recv().is_err());
    drop(lease);
    released
        .recv_timeout(Duration::from_secs(3))
        .unwrap()
        .unwrap();
    assert!(runtime.snapshot().active.is_none());
}
