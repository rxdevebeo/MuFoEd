use super::*;
use std::{sync::atomic::AtomicUsize, time::Instant};

#[derive(Default)]
struct Mock {
    resident: AtomicBool,
    release: AtomicBool,
    calls: Mutex<Vec<String>>,
    unloads: AtomicUsize,
    wrong_ack: AtomicBool,
    runtime: Mutex<Option<Runtime>>,
}
impl Transport for Mock {
    fn request(&self, _: &str, path: &str, body: Option<&Value>) -> Result<Value, String> {
        self.calls.lock().unwrap().push(path.into());
        match path {
            "/api/tags" => {
                Ok(json!({"models": [{"name": "test:latest", "digest": "weights-digest"}]}))
            }
            "/api/ps" => Ok(if self.resident.load(Ordering::Acquire) {
                json!({"models": [{"name": "another-alias:latest", "digest": "weights-digest"}]})
            } else {
                json!({"models": []})
            }),
            "/api/generate" => {
                assert!(matches!(
                    self.runtime
                        .lock()
                        .unwrap()
                        .as_ref()
                        .unwrap()
                        .snapshot()
                        .phase,
                    Some(Phase::Releasing | Phase::Unknown)
                ));
                self.unloads.fetch_add(1, Ordering::AcqRel);
                assert_eq!(body.unwrap()["model"], "test");
                assert_eq!(body.unwrap()["keep_alive"], 0);
                assert!(body.unwrap().get("prompt").is_none());
                if self.release.load(Ordering::Acquire) {
                    self.resident.store(false, Ordering::Release);
                    Ok(
                        json!({"done": true, "done_reason": if self.wrong_ack.load(Ordering::Acquire) { "stop" } else { "unload" }}),
                    )
                } else {
                    Err("simulated broken connection".into())
                }
            }
            _ => panic!("Unexpected endpoint: {path}"),
        }
    }
}
fn setup() -> (Runtime, Arc<Mock>, Client) {
    let runtime = Runtime::new(Policy::default());
    let mock = Arc::new(Mock::default());
    *mock.runtime.lock().unwrap() = Some(runtime.clone());
    mock.release.store(true, Ordering::Release);
    let client = Client::new("http://localhost:11434", "test", &runtime)
        .unwrap()
        .with_transport(mock.clone());
    (runtime, mock, client)
}
fn wait_released(runtime: &Runtime) {
    let start = Instant::now();
    while runtime.snapshot().active.is_some() {
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn owned_inference_blocks_images_until_confirmed_unload() {
    let (runtime, mock, client) = setup();
    client
        .run("text", &AtomicBool::new(false), || {
            assert_eq!(runtime.snapshot().phase, Some(Phase::Running));
            mock.resident.store(true, Ordering::Release);
            let mut image = runtime.enqueue(Task::new("image", "plakat")).unwrap();
            assert!(image.try_start().unwrap().is_none());
            assert!(owned_models()
                .iter()
                .any(|m| m.model == "test" && m.phase == Phase::Running));
            Ok(())
        })
        .unwrap();
    assert_eq!(mock.unloads.load(Ordering::Acquire), 1);
    assert!(!mock.resident.load(Ordering::Acquire));
    assert!(runtime.snapshot().active.is_none());
}

#[test]
fn foreign_residency_including_digest_alias_is_never_adopted_or_unloaded() {
    let (runtime, mock, client) = setup();
    mock.resident.store(true, Ordering::Release);
    let result: Result<(), _> =
        client.run("text", &AtomicBool::new(false), || panic!("must not infer"));
    assert!(result.unwrap_err().contains("outside this manager"));
    assert_eq!(mock.unloads.load(Ordering::Acquire), 0);
    assert!(runtime.snapshot().active.is_none());
    assert!(mock.resident.load(Ordering::Acquire));
}

#[test]
fn failed_http_retains_admission_until_recovery_confirms_release() {
    let (runtime, mock, client) = setup();
    mock.release.store(false, Ordering::Release);
    let result: Result<(), _> = client.run("vision", &AtomicBool::new(false), || {
        mock.resident.store(true, Ordering::Release);
        Err("inference connection lost".into())
    });
    assert!(result.unwrap_err().contains("admission retained"));
    assert_eq!(runtime.snapshot().phase, Some(Phase::Unknown));
    let mut image = runtime.enqueue(Task::new("image", "next")).unwrap();
    assert!(image.try_start().unwrap().is_none());
    mock.release.store(true, Ordering::Release);
    wait_released(&runtime);
    assert!(image.try_start().unwrap().is_some());
}

#[test]
fn absence_without_unload_acknowledgement_does_not_release_admission() {
    let (runtime, mock, client) = setup();
    mock.wrong_ack.store(true, Ordering::Release);
    let result = client.run("text", &AtomicBool::new(false), || Ok(()));
    assert!(result.unwrap_err().contains("unconfirmed"));
    assert!(!mock.resident.load(Ordering::Acquire));
    assert_eq!(runtime.snapshot().phase, Some(Phase::Unknown));
    mock.wrong_ack.store(false, Ordering::Release);
    wait_released(&runtime);
}

#[test]
fn callback_error_is_preserved_after_successful_cleanup() {
    let (runtime, _, client) = setup();
    let result: Result<(), _> = client.run("text", &AtomicBool::new(false), || {
        Err("original backend error".into())
    });
    assert_eq!(result.unwrap_err(), "original backend error");
    assert!(runtime.snapshot().active.is_none());
}

#[test]
fn callback_panic_transfers_release_ownership_to_recovery_worker() {
    let (runtime, mock, client) = setup();
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _: Result<(), _> = client.run("vision", &AtomicBool::new(false), || {
            mock.resident.store(true, Ordering::Release);
            panic!("backend panic");
        });
    }));
    assert!(panic.is_err());
    wait_released(&runtime);
    assert!(!mock.resident.load(Ordering::Acquire));
}

#[test]
fn cancelling_while_queued_never_contacts_or_unloads_a_server_model() {
    let (runtime, mock, client) = setup();
    let mut image = runtime.enqueue(Task::new("image", "active")).unwrap();
    let _lease = image.try_start().unwrap().unwrap();
    let flag = Arc::new(AtomicBool::new(false));
    let worker_flag = flag.clone();
    let worker = std::thread::spawn(move || client.run("text", &worker_flag, || Ok(())));
    let start = Instant::now();
    while runtime.snapshot().queued.is_empty() {
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
    flag.store(true, Ordering::Release);
    assert!(worker.join().unwrap().is_err());
    assert!(runtime.snapshot().queued.is_empty());
    assert!(mock.calls.lock().unwrap().is_empty());
}

#[test]
fn remote_endpoints_have_separate_domains_and_normalized_identity() {
    let local = Runtime::new(Policy::default());
    let mut local_task = local.enqueue(Task::new("image", "local")).unwrap();
    let _lease = local_task.try_start().unwrap().unwrap();
    let a = Client::new("https://example.invalid/", "test", &local).unwrap();
    let b = Client::new("https://EXAMPLE.invalid:443", "test", &local).unwrap();
    let mut one = a.runtime().enqueue(Task::new("text", "remote")).unwrap();
    let _remote = one.try_start().unwrap().unwrap();
    let mut two = b.runtime().enqueue(Task::new("vision", "remote")).unwrap();
    assert!(two.try_start().unwrap().is_none());
    assert_eq!(local.snapshot().active.unwrap().owner, "image");
}

#[test]
fn endpoint_validation_rejects_credentials_and_keeps_remote_memory_separate() {
    for endpoint in [
        "ftp://localhost",
        "http://user:secret@localhost",
        "http://localhost?token=secret",
    ] {
        assert!(endpoint_domain(endpoint).is_err());
    }
    assert!(endpoint_domain("http://[::1]:11434").unwrap().1);
    assert!(endpoint_domain("http://127.0.0.2:11434").unwrap().1);
    assert!(!endpoint_domain("http://192.0.2.1:11434").unwrap().1);
}
