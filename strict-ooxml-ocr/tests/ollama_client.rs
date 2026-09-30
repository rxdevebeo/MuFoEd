//! The ollama client, against a server the test runs itself.
//!
//! The point of a mock is that the test does not need a model, a GPU or a
//! network: it answers the two calls the client makes and the test asserts on
//! what the client *sends* and on what it does with the answer. A client that
//! only works against a real daemon is a client nobody can test the error paths
//! of, and the error paths are the ones that matter (O5).

#![cfg(feature = "ocr-ollama")]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;

use strict_ooxml_ocr::ollama::{OllamaConfig, OllamaVision};
use strict_ooxml_ocr::traits::FigureClassifier;
use strict_ooxml_ocr::{Image, VisionError};

/// One request the mock server saw.
#[derive(Clone, Debug)]
struct Seen {
    path: String,
    body: String,
}

/// A server that answers `/api/show` and `/api/chat` and records what it saw.
struct Mock {
    endpoint: String,
    seen: mpsc::Receiver<Seen>,
    _handle: thread::JoinHandle<()>,
}

impl Mock {
    /// Starts a server; `chat_body` is what `/api/chat` answers with.
    fn start(chat_body: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a local port");
        let endpoint = format!("http://{}", listener.local_addr().expect("addr"));
        let (sender, seen) = mpsc::channel();
        let handle = thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(match stream.try_clone() {
                    Ok(clone) => clone,
                    Err(_) => continue,
                });
                let mut request_line = String::new();
                if reader.read_line(&mut request_line).is_err() {
                    continue;
                }
                let path = request_line
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("")
                    .to_owned();
                let mut length = 0usize;
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).is_err() || header.trim().is_empty() {
                        break;
                    }
                    if let Some((name, value)) = header.split_once(':') {
                        if name.trim().eq_ignore_ascii_case("content-length") {
                            length = value.trim().parse().unwrap_or(0);
                        }
                    }
                }
                let mut body = vec![0u8; length];
                if length > 0 && reader.read_exact(&mut body).is_err() {
                    continue;
                }
                let body = String::from_utf8_lossy(&body).into_owned();
                let _ = sender.send(Seen {
                    path: path.clone(),
                    body,
                });
                let payload = if path == "/api/show" {
                    "{\"details\":{\"digest\":\"sha256:abc123\"}}"
                } else {
                    chat_body
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    payload.len(),
                    payload
                );
                let _ = stream.flush();
            }
        });
        Self {
            endpoint,
            seen,
            _handle: handle,
        }
    }
}

fn config(endpoint: &str) -> OllamaConfig {
    OllamaConfig {
        endpoint: endpoint.to_owned(),
        model: "test-vision".to_owned(),
        ..OllamaConfig::default()
    }
}

fn image() -> Image {
    Image::png(b"\x89PNG\r\n\x1a\nfake".to_vec())
}

#[test]
fn a_figure_is_described_and_the_answer_carries_its_model() {
    let mock = Mock::start(
        r#"{"message":{"content":"diagram\nA bar chart of revenue by quarter."},"model":"test-vision"}"#,
    );
    let client = OllamaVision::new(config(&mock.endpoint));
    let answer = client
        .describe(&image(), "page 1")
        .expect("the model answered");
    assert_eq!(answer.model, "test-vision");
    assert_eq!(answer.version, "sha256:abc123");
    assert!(answer.text.starts_with("diagram"), "{}", answer.text);

    // The version call comes first, then the chat.
    let first = mock.seen.recv().expect("a request");
    assert_eq!(first.path, "/api/show");
    assert!(first.body.contains("test-vision"), "{}", first.body);
    let second = mock.seen.recv().expect("a request");
    assert_eq!(second.path, "/api/chat");
    assert!(second.body.contains("\"stream\":false"), "{}", second.body);
    assert!(second.body.contains("\"images\":["), "{}", second.body);
    // The PNG signature in base64: 89 50 4E 47 -> "iVBOR".
    assert!(
        second.body.contains("iVBOR"),
        "the image must be base64: {}",
        second.body
    );
    assert!(second.body.contains("page 1"), "{}", second.body);
}

#[test]
fn the_request_carries_no_retry_and_no_temperature_above_zero() {
    let mock = Mock::start(r#"{"message":{"content":"illustration\nA photo."}}"#);
    let client = OllamaVision::new(OllamaConfig {
        temperature: 0.0,
        ..config(&mock.endpoint)
    });
    let _ = client.describe(&image(), "").expect("the model answered");
    let _ = mock.seen.recv().expect("a request");
    let chat = mock.seen.recv().expect("a request");
    assert!(chat.body.contains("\"temperature\":0"), "{}", chat.body);
    assert!(!chat.body.contains("\"stream\":true"), "{}", chat.body);
}

#[test]
fn a_daemon_that_is_not_there_is_an_unreachable_error() {
    // Port 1 on the loopback interface: nothing listens there.
    let client = OllamaVision::new(OllamaConfig {
        endpoint: "http://127.0.0.1:1".to_owned(),
        ..OllamaConfig::default()
    });
    let error = client
        .describe(&image(), "")
        .expect_err("there is no daemon on port 1");
    assert!(
        matches!(error, VisionError::Unreachable(_) | VisionError::TimedOut),
        "{error}"
    );
    // The message has to say what went wrong: a caller seeing only "error" cannot
    // tell a missing daemon from a refused request.
    assert!(error.to_string().contains("unreachable"), "{error}");
}

#[test]
fn an_answer_with_no_content_is_unusable_not_empty() {
    let mock = Mock::start(r#"{"message":{"content":"   "}}"#);
    let client = OllamaVision::new(config(&mock.endpoint));
    let error = client
        .describe(&image(), "")
        .expect_err("a blank answer is not an answer");
    assert!(matches!(error, VisionError::Unusable(_)), "{error}");
    assert!(error.to_string().contains("no content"), "{error}");
}

#[test]
fn an_answer_that_is_not_json_is_rejected() {
    let mock = Mock::start("<html>the daemon is unhappy</html>");
    let client = OllamaVision::new(config(&mock.endpoint));
    let error = client
        .describe(&image(), "")
        .expect_err("HTML is not an answer");
    assert!(error.to_string().contains("not JSON"), "{error}");
}

#[test]
fn the_response_budget_is_applied() {
    let mock = Mock::start(
        r#"{"message":{"content":"diagram\nA very long description that goes on and on."}}"#,
    );
    let client = OllamaVision::new(OllamaConfig {
        max_response_bytes: 16,
        ..config(&mock.endpoint)
    });
    let error = client
        .describe(&image(), "")
        .expect_err("a 16 byte budget cannot hold this");
    // The budget is enforced by the transport, so the wording comes from there;
    // what matters is that the answer was refused rather than truncated into
    // something that parses.
    let text = error.to_string();
    assert!(text.contains("budget") || text.contains("limit"), "{text}");
}

#[test]
fn the_client_does_not_depend_on_a_model_being_installed() {
    // The configuration is what decides; a model nobody has is a *runtime*
    // failure, not a compile-time one, and this test must keep working on a
    // machine with no models at all.
    let client = OllamaVision::from_env();
    assert!(!client.model_name().is_empty());
    assert!(client.config().chat_url().ends_with("/api/chat"));
}
