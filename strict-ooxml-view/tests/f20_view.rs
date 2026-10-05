//! A08 / F20: a successful view still shows the normalization loss.

use std::io::{BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn get(port: u16, path: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(60)))
        .expect("timeout");
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )
    .expect("write");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read");
    let (head, body) = response.split_once("\r\n\r\n").unwrap_or(("", &response));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or(0);
    (status, body.to_owned())
}

#[test]
fn f20_successful_view_exposes_normalization_loss() {
    let dir = std::env::temp_dir().join(format!("strict-view-f20-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let docx = dir.join("vml-loss.docx");
    std::fs::write(&docx, strict_ooxml_testkit::audit::vml_loss_docx()).expect("docx");
    // A sidecar for a different file must not turn this loss into a clean view.
    std::fs::write(
        dir.join("vml-loss.docx.report.json"),
        r#"{"version":1,"output_sha256":"0000000000000000000000000000000000000000000000000000000000000000","outcome":"clean","issues":[]}"#,
    )
    .expect("sidecar");

    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        drop(listener);
        port
    };
    let mut child = Command::new(env!("CARGO_BIN_EXE_strict-ooxml-view"))
        .arg(&dir)
        .args(["--port", &port.to_string(), "--transitional"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn viewer");
    let stdout = child.stdout.take().expect("stdout");
    let stderr = child.stderr.take().expect("stderr");
    let mut server = Server(child);
    let mut ready = false;
    for _ in 0..50 {
        if server.0.try_wait().ok().flatten().is_some() {
            break;
        }
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            ready = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if !ready {
        let mut output = String::new();
        let _ = BufReader::new(stdout).read_to_string(&mut output);
        let mut error = String::new();
        let _ = BufReader::new(stderr).read_to_string(&mut error);
        panic!("viewer did not accept connections\nstdout={output}\nstderr={error}");
    }

    let (status, page) = get(port, "/");
    assert_eq!(status, 200, "the page itself is served");
    assert!(
        page.contains("id=\"losses\""),
        "the shell has a loss summary"
    );
    assert!(
        page.contains("dataset.outcome"),
        "the script publishes the pipeline outcome"
    );

    let (status, body) = get(port, "/api/document?name=vml-loss.docx");
    assert_eq!(status, 200, "a lossy document still opens");
    let ledger = body.split("\"pages\"").next().unwrap_or(&body);
    assert!(
        ledger.contains("\"outcome\":\"degraded\""),
        "the live outcome is degraded: {ledger}"
    );
    assert!(
        ledger.contains("\"stage\":\"normalize\""),
        "the loss is from normalization: {ledger}"
    );
    assert!(
        ledger.contains("VML") || ledger.contains("dropped"),
        "the reason is in the API: {ledger}"
    );
    assert!(
        ledger.contains("\"sidecar\":\"rejected\""),
        "the stale sidecar must not replace the live report: {ledger}"
    );
    assert!(
        body.contains("KEEP") && body.contains("THIS") && body.contains("TEXT"),
        "the surviving text is in the rendered page"
    );
    assert!(
        ledger.contains("\"status\":\"not_run\""),
        "convert and write did not run in the viewer: {ledger}"
    );
    if std::env::var_os("F20_HOLD").is_some() {
        println!("F20_URL http://127.0.0.1:{port}/");
        std::thread::park();
    }
    drop(server);
    let _ = std::fs::remove_dir_all(&dir);
}
