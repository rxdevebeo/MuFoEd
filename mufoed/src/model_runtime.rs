//! Application adapters to the common text/vision/image model coordinator.
use mufoed_model_runtime::{ollama::Client, Runtime};
use std::sync::atomic::AtomicBool;

fn runtime() -> Runtime {
    #[cfg(not(test))]
    {
        Runtime::global()
    }
    #[cfg(test)]
    {
        struct Physical;
        impl mufoed_model_runtime::telemetry::Telemetry for Physical {
            fn observe(&self) -> mufoed_model_runtime::telemetry::Observation {
                mufoed_model_runtime::telemetry::Observation {
                    at: std::time::Instant::now(),
                    ram_total: Some(64 << 30),
                    ram_available: Some(60 << 30),
                    gpus: vec![mufoed_model_runtime::telemetry::Gpu {
                        uuid: "GPU-fixture".into(),
                        ordinal: 0,
                        total: 16 << 30,
                        free: 14 << 30,
                    }],
                    diagnostic: None,
                }
            }
        }
        Runtime::managed(
            mufoed_model_runtime::Policy::default(),
            mufoed_model_runtime::residency::MemoryPolicy::default(),
            std::sync::Arc::new(Physical),
        )
    }
}

/// Execute on a worker and retain admission until Ollama confirms model release.
pub(crate) fn run<T>(
    endpoint: &str,
    model: &str,
    owner: &str,
    cancel: &AtomicBool,
    context: u32,
    operation: impl FnMut() -> Result<T, String>,
) -> Result<T, String> {
    Client::new(endpoint, model, &runtime())?
        .with_context(context)
        .run(owner, cancel, operation)
}

/// Wrap the existing library recognizer without changing its prompts or parsing contracts.
#[cfg(feature = "ocr")]
pub(crate) struct ManagedVision {
    backend: strict_ooxml::OllamaVision,
}
#[cfg(feature = "ocr")]
impl ManagedVision {
    pub(crate) fn new(config: strict_ooxml::OllamaConfig) -> Self {
        Self {
            backend: strict_ooxml::OllamaVision::new(config),
        }
    }
    fn call<T>(
        &self,
        mut operation: impl FnMut(&strict_ooxml::OllamaVision) -> Result<T, strict_ooxml::VisionError>,
    ) -> Result<T, strict_ooxml::VisionError> {
        let config = self.backend.config();
        let client = Client::new(&config.endpoint, &config.model, &runtime())
            .map_err(strict_ooxml::VisionError::Unreachable)?
            .with_context(config.context_tokens);
        let mut cause = None;
        let result = client.run("vision", &AtomicBool::new(false), || {
            match operation(&self.backend) {
                Ok(value) => {
                    cause = None;
                    Ok(value)
                }
                Err(error) => {
                    let message = error.to_string();
                    cause = Some(error);
                    Err(message)
                }
            }
        });
        match result {
            Ok(value) => Ok(value),
            Err(message) => match cause {
                Some(error) if error.to_string() == message => Err(error),
                _ => Err(strict_ooxml::VisionError::Unreachable(message)),
            },
        }
    }
}
#[cfg(feature = "ocr")]
impl strict_ooxml_ocr::TextRecovery for ManagedVision {
    fn model_name(&self) -> &str {
        &self.backend.config().model
    }
    fn recover_page(
        &self,
        page: &strict_ooxml_ocr::Image,
    ) -> Result<Option<strict_ooxml_ocr::Recovered>, strict_ooxml_ocr::VisionError> {
        self.call(|backend| backend.recover_page(page))
    }
    fn recover_region(
        &self,
        page: &strict_ooxml_ocr::Image,
    ) -> Result<Option<strict_ooxml_ocr::Recovered>, strict_ooxml_ocr::VisionError> {
        self.call(|backend| backend.recover_region(page))
    }
    fn recover_table(
        &self,
        page: &strict_ooxml_ocr::Image,
    ) -> Result<Option<strict_ooxml_ocr::Recovered>, strict_ooxml_ocr::VisionError> {
        self.call(|backend| backend.recover_table(page))
    }
    fn recover_formula(
        &self,
        page: &strict_ooxml_ocr::Image,
    ) -> Result<Option<strict_ooxml_ocr::Recovered>, strict_ooxml_ocr::VisionError> {
        self.call(|backend| backend.recover_formula(page))
    }
    fn recover_layout(
        &self,
        page: &strict_ooxml_ocr::Image,
    ) -> Result<Option<strict_ooxml_ocr::Layout>, strict_ooxml_ocr::VisionError> {
        self.call(|backend| backend.recover_layout(page))
    }
}
#[cfg(feature = "ocr")]
impl strict_ooxml_ocr::FigureClassifier for ManagedVision {
    fn model_name(&self) -> &str {
        &self.backend.config().model
    }
    fn describe(
        &self,
        page: &strict_ooxml_ocr::Image,
        context: &str,
    ) -> Result<strict_ooxml_ocr::Recovered, strict_ooxml_ocr::VisionError> {
        self.call(|backend| strict_ooxml_ocr::FigureClassifier::describe(backend, page, context))
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use serde_json::{json, Value};
    use std::{
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        time::{Duration, Instant},
    };

    /// A complete owned-model HTTP lifecycle, retaining the caller's content assertions.
    pub(crate) fn server(
        listener: TcpListener,
        model: &'static str,
        mut chat: impl FnMut(Value) -> Value + Send + 'static,
    ) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            listener.set_nonblocking(true).unwrap();
            let began = Instant::now();
            let mut released = false;
            loop {
                assert!(
                    began.elapsed() < Duration::from_secs(15),
                    "model lifecycle HTTP fixture timed out"
                );
                let (mut stream, _) = match listener.accept() {
                    Ok(stream) => stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(error) => panic!("{error}"),
                };
                stream.set_nonblocking(false).unwrap();
                let (path, body) = read(&mut stream);
                let response = match path.as_str() {
                    "/api/tags" => json!({"models": [{"name": model, "digest": "fixture-digest", "size":1024}]}),
                    "/api/ps" => json!({"models": []}),
                    "/api/show" => json!({"details": {"digest": "fixture-digest"}, "model_info":{"llama.block_count":2,"llama.embedding_length":8,"llama.attention.head_count":2,"llama.attention.head_count_kv":1,"llama.context_length":8192}}),
                    "/api/chat" => {
                        assert_eq!(body["model"], model);
                        chat(body)
                    }
                    "/api/generate" => {
                        assert_eq!(body["model"], model);
                        assert_eq!(body["keep_alive"], 0);
                        assert!(body.get("prompt").is_none());
                        released = true;
                        json!({"done": true, "done_reason": "unload"})
                    }
                    _ => panic!("Unexpected endpoint {path}"),
                }
                .to_string();
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).unwrap();
                if released && path == "/api/ps" {
                    break;
                }
            }
        })
    }
    fn read(stream: &mut TcpStream) -> (String, Value) {
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut bytes = Vec::new();
        loop {
            let mut buffer = [0; 4096];
            let count = stream.read(&mut buffer).unwrap();
            assert_ne!(count, 0);
            bytes.extend_from_slice(&buffer[..count]);
            let Some(end) = bytes.windows(4).position(|b| b == b"\r\n\r\n") else {
                continue;
            };
            let headers = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
            let length = headers
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .map_or(0, |value| value.trim().parse::<usize>().unwrap());
            if bytes.len() >= end + 4 + length {
                let path = headers
                    .lines()
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .to_owned();
                let body = if length == 0 {
                    Value::Null
                } else {
                    serde_json::from_slice(&bytes[end + 4..end + 4 + length]).unwrap()
                };
                return (path, body);
            }
        }
    }
}
