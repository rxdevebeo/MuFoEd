//! A minimal blocking HTTP/1.1 server for the viewer.
//!
//! Hand-rolled on `std::net::TcpListener` rather than pulled from a web
//! framework: the server answers the viewer's routes on loopback, serves bytes that
//! are already in memory, and shares the process with the renderer. A
//! framework would add a dependency tree to a workspace whose licences and
//! advisories are checked by `cargo deny` (`deny.toml`), for a loopback tool.
//!
//! What it does implement, because a browser needs it and a hostile client
//! must not hang the process (AUD-16): one request per connection, a bounded
//! request line and headers, a body ceiling, and read/write timeouts.
//! `Content-Length` on every response; always `Connection: close`.
//!
//! And one check a loopback server needs that a public one would not: the
//! `Host` header must name loopback. Binding to 127.0.0.1 keeps other machines
//! out, but not a web page in the user's own browser — a hostile site whose DNS
//! name is re-pointed at 127.0.0.1 (DNS rebinding) is then *same-origin* with
//! this server and can read the corpus. Its requests carry its own name in
//! `Host`, so a request for any other host is answered `421` and never reaches
//! a route.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Longest request line or header line accepted, in bytes (AUD-16).
const MAX_LINE_BYTES: usize = 8 * 1024;
/// Most header fields accepted before the blank line (AUD-16).
const MAX_HEADERS: usize = 100;
/// Largest request body accepted, in bytes (AUD-16).
const MAX_BODY_BYTES: u64 = 1024 * 1024;
/// Idle and write budget for one connection (AUD-16).
const IO_TIMEOUT: Duration = Duration::from_secs(10);

/// A response the server should send.
#[derive(Debug)]
pub(crate) struct Response {
    /// HTTP status code.
    pub status: u16,
    /// `Content-Type` header value.
    pub content_type: &'static str,
    /// Response body.
    pub body: Vec<u8>,
}

impl Response {
    /// A `200` carrying `body` as `content_type`.
    #[must_use]
    pub(crate) fn ok(content_type: &'static str, body: Vec<u8>) -> Self {
        Self {
            status: 200,
            content_type,
            body,
        }
    }

    /// A `404` with a plain-text body.
    #[must_use]
    pub(crate) fn not_found(what: &str) -> Self {
        Self {
            status: 404,
            content_type: "text/plain; charset=utf-8",
            body: format!("not found: {what}").into_bytes(),
        }
    }

    /// A `400` with a plain-text body.
    #[must_use]
    pub(crate) fn bad_request(message: &str) -> Self {
        Self {
            status: 400,
            content_type: "text/plain; charset=utf-8",
            body: message.as_bytes().to_vec(),
        }
    }

    /// A `413` when `Content-Length` is over the body ceiling.
    #[must_use]
    pub(crate) fn payload_too_large() -> Self {
        Self {
            status: 413,
            content_type: "text/plain; charset=utf-8",
            body: b"payload too large".to_vec(),
        }
    }

    /// A `431` when the request line or headers do not fit the budget.
    #[must_use]
    pub(crate) fn header_fields_too_large() -> Self {
        Self {
            status: 431,
            content_type: "text/plain; charset=utf-8",
            body: b"request header fields too large".to_vec(),
        }
    }

    /// A `421` when `Host` does not name this loopback server (DNS rebinding).
    #[must_use]
    pub(crate) fn misdirected() -> Self {
        Self {
            status: 421,
            content_type: "text/plain; charset=utf-8",
            body: b"misdirected request: Host must be 127.0.0.1, localhost or [::1]".to_vec(),
        }
    }

    /// The reason phrase for the status.
    const fn reason(&self) -> &'static str {
        match self.status {
            400 => "Bad Request",
            404 => "Not Found",
            413 => "Payload Too Large",
            421 => "Misdirected Request",
            431 => "Request Header Fields Too Large",
            _ => "OK",
        }
    }
}

/// Percent-decodes a URL component.
///
/// Returns `None` on malformed input, which the caller turns into a 400
/// rather than guessing: a component that does not decode is a client bug, and
/// it must not become a file name.
#[must_use]
pub(crate) fn percent_decode(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        if byte == b'%' {
            let hex = input.get(index + 1..index + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else if byte == b'+' {
            out.push(b' ');
            index += 1;
        } else {
            out.push(byte);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Whether a `Host` header value names this server on loopback.
///
/// The names a browser sends for a page it loaded from this server:
/// `127.0.0.1`, `localhost` or `[::1]`, with this server's `port` or with no
/// port at all (which a browser only omits for port 80). Anything else —
/// another name, another port, a missing header — is a request that was not
/// meant for this server, or was meant for it by somebody else's page.
#[must_use]
pub(crate) fn host_allowed(host: Option<&str>, port: u16) -> bool {
    let Some(host) = host.map(str::trim) else {
        return false;
    };
    ["127.0.0.1", "localhost", "[::1]"].iter().any(|name| {
        host.eq_ignore_ascii_case(name) || host.eq_ignore_ascii_case(&format!("{name}:{port}"))
    })
}

/// Serves `handler` on `listener` until `stop` is set.
///
/// `handler` receives the decoded path and query and returns a [`Response`].
/// A request whose `Host` does not name this server on loopback is answered
/// `421` without reaching `handler` ([`host_allowed`]).
/// The loop is single-threaded and deliberately so: a viewer is opened by one
/// person at a time, and one slow render must not interleave with a page
/// request. Each connection answers at most one request and then closes
/// (AUD-16).
pub(crate) fn serve<F>(listener: &TcpListener, stop: &AtomicBool, handler: F) -> std::io::Result<()>
where
    F: Fn(&str, &str) -> Response,
{
    let port = listener.local_addr()?.port();
    listener.set_nonblocking(true)?;
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                // A single failed connection must not stop the server: a
                // browser that cancels a request mid-body is routine.
                let _ = handle_connection(stream, port, &handler);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// Handles one connection: timeouts on, one request, then close.
fn handle_connection<F>(stream: TcpStream, port: u16, handler: &F) -> std::io::Result<()>
where
    F: Fn(&str, &str) -> Response,
{
    // The listener is non-blocking so `serve` can poll `stop`; the accepted
    // stream must be blocking again or read timeouts never arm and a
    // WouldBlock looks like an idle close (AUD-16).
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    let write_half = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    let mut writer = write_half;
    let response = match read_request(&mut reader)? {
        ReadOutcome::Closed => return Ok(()),
        ReadOutcome::Rejected(response) => response,
        ReadOutcome::Ok(request) if !host_allowed(request.host.as_deref(), port) => {
            Response::misdirected()
        }
        ReadOutcome::Ok(request) => handler(&request.path, &request.query),
    };
    write_response(&mut writer, &response)
}

/// A parsed request line.
#[derive(Debug)]
struct Request {
    /// Decoded path.
    path: String,
    /// Decoded query string.
    query: String,
    /// The `Host` header, when the request sent one.
    host: Option<String>,
}

/// Result of reading one HTTP request head.
#[derive(Debug)]
enum ReadOutcome {
    /// Peer closed before sending a request line.
    Closed,
    /// A usable GET/HEAD request.
    Ok(Request),
    /// Head or body over budget; caller sends the status and closes.
    Rejected(Response),
}

/// Reads a request line, headers, and refuses an oversized body.
///
/// Returns [`ReadOutcome::Closed`] at end of stream. A line over 8 KiB or more
/// than 100 headers becomes `431`; a `Content-Length` over 1 MiB becomes `413`
/// (AUD-16).
fn read_request(reader: &mut impl BufRead) -> std::io::Result<ReadOutcome> {
    let line = match read_line(reader)? {
        LineRead::Eof => return Ok(ReadOutcome::Closed),
        LineRead::TooLarge => {
            return Ok(ReadOutcome::Rejected(Response::header_fields_too_large()));
        }
        LineRead::Line(line) => line,
    };
    let request = String::from_utf8_lossy(&line).into_owned();
    let mut parts = request.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default().to_owned();
    let supported = method.eq_ignore_ascii_case("GET") || method.eq_ignore_ascii_case("HEAD");

    let mut header_count = 0usize;
    let mut content_length: Option<u64> = None;
    let mut host: Option<String> = None;
    loop {
        let header_line = match read_line(reader)? {
            LineRead::Eof => break,
            LineRead::TooLarge => {
                return Ok(ReadOutcome::Rejected(Response::header_fields_too_large()));
            }
            LineRead::Line(line) => line,
        };
        if header_line == b"\r\n" || header_line == b"\n" {
            break;
        }
        header_count += 1;
        if header_count > MAX_HEADERS {
            return Ok(ReadOutcome::Rejected(Response::header_fields_too_large()));
        }
        let header = String::from_utf8_lossy(&header_line);
        if let Some((name, value)) = header.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().ok();
            } else if name.eq_ignore_ascii_case("host") {
                host = Some(value.trim().to_owned());
            }
        }
    }

    if let Some(length) = content_length {
        if length > MAX_BODY_BYTES {
            return Ok(ReadOutcome::Rejected(Response::payload_too_large()));
        }
        // Discard a body we will not use; the connection closes after the
        // response either way, but leaving unread bytes would confuse a test
        // that reuses a Cursor.
        let mut sink = std::io::sink();
        std::io::copy(&mut reader.take(length), &mut sink)?;
    }

    if !supported {
        return Ok(ReadOutcome::Ok(Request {
            path: "/__method".to_owned(),
            query: String::new(),
            host,
        }));
    }
    let (path, query) = match target.split_once('?') {
        Some((path, query)) => (path.to_owned(), query.to_owned()),
        None => (target, String::new()),
    };
    Ok(ReadOutcome::Ok(Request {
        path: percent_decode(&path).unwrap_or_else(|| "/".to_owned()),
        query: percent_decode(&query).unwrap_or_default(),
        host,
    }))
}

/// Outcome of reading one capped request or header line.
enum LineRead {
    /// Peer closed before any byte of this line.
    Eof,
    /// The line exceeded [`MAX_LINE_BYTES`].
    TooLarge,
    /// A complete line, including the terminating LF.
    Line(Vec<u8>),
}

/// Reads one line capped at [`MAX_LINE_BYTES`] via `Read::take` (AUD-16).
fn read_line(reader: &mut impl BufRead) -> std::io::Result<LineRead> {
    let mut out = Vec::new();
    // One extra byte past the ceiling so a line that fills the take without a
    // newline is distinguishable from a legal line that ends exactly at 8 KiB.
    let mut limited = reader.take(u64::try_from(MAX_LINE_BYTES + 1).unwrap_or(u64::MAX));
    let read = limited.read_until(b'\n', &mut out)?;
    if read == 0 {
        return Ok(LineRead::Eof);
    }
    if out.len() > MAX_LINE_BYTES {
        // The take was filled without a newline: the line is over budget.
        return Ok(LineRead::TooLarge);
    }
    if !out.ends_with(b"\n") {
        // Peer closed mid-line; not an oversize attack.
        return Ok(LineRead::Eof);
    }
    Ok(LineRead::Line(out))
}

/// Writes a response and always closes the connection (AUD-16).
fn write_response(writer: &mut impl Write, response: &Response) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.status,
        response.reason(),
        response.content_type,
        response.body.len(),
    );
    writer.write_all(head.as_bytes())?;
    writer.write_all(&response.body)?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    use super::{
        host_allowed, percent_decode, read_request, serve, ReadOutcome, Response, IO_TIMEOUT,
        MAX_HEADERS, MAX_LINE_BYTES,
    };

    #[test]
    fn plain_text_round_trips() {
        assert_eq!(percent_decode("hello").as_deref(), Some("hello"));
    }

    #[test]
    fn a_non_ascii_name_survives_the_round_trip() {
        let name = "%D0%B4%D0%BE%D0%BA.docx";
        assert_eq!(percent_decode(name).as_deref(), Some("док.docx"));
    }

    #[test]
    fn percent_and_plus_decode_as_the_query_specifies() {
        assert_eq!(percent_decode("a%20b%2Fc").as_deref(), Some("a b/c"));
        assert_eq!(percent_decode("a+b").as_deref(), Some("a b"));
    }

    #[test]
    fn malformed_escapes_are_rejected_rather_than_guessed() {
        assert_eq!(percent_decode("%zz"), None);
        assert_eq!(percent_decode("%4"), None);
        assert_eq!(percent_decode("%"), None);
        // %FF is not a valid UTF-8 lead byte on its own.
        assert_eq!(percent_decode("%FF"), None);
    }

    #[test]
    fn a_get_request_line_is_split_into_path_and_query() {
        let raw = b"GET /api/document?name=a.docx HTTP/1.1\r\nHost: x\r\n\r\n";
        let mut reader = std::io::Cursor::new(raw.to_vec());
        let ReadOutcome::Ok(request) = read_request(&mut reader).expect("read") else {
            panic!("expected a request");
        };
        assert_eq!(request.path, "/api/document");
        assert_eq!(request.query, "name=a.docx");
    }

    #[test]
    fn a_decoded_name_reaches_the_handler_intact() {
        let raw = "GET /api/document?name=%D0%B4%D0%BE%D0%BA.docx HTTP/1.1\r\n\r\n";
        let mut reader = std::io::Cursor::new(raw.as_bytes().to_vec());
        let ReadOutcome::Ok(request) = read_request(&mut reader).expect("read") else {
            panic!("expected a request");
        };
        assert_eq!(request.query, "name=док.docx");
    }

    #[test]
    fn end_of_stream_ends_the_connection() {
        let mut reader = std::io::Cursor::new(Vec::new());
        assert!(matches!(
            read_request(&mut reader).expect("read"),
            ReadOutcome::Closed
        ));
    }

    #[test]
    fn a_response_always_closes_the_connection() {
        let response = Response::ok("text/plain", b"x".to_vec());
        let mut out = Vec::new();
        super::write_response(&mut out, &response).expect("write");
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.contains("Connection: close"));
        assert!(!text.to_ascii_lowercase().contains("keep-alive"));
    }

    #[test]
    fn a_hundred_kib_request_line_is_refused_with_431() {
        // AUD-16: a single line past 8 KiB must not allocate unboundedly and
        // must surface as 431 (or close); 100 KiB is the plan's probe size.
        let mut raw = Vec::new();
        raw.extend_from_slice(b"GET /");
        raw.resize(raw.len() + 100 * 1024, b'a');
        raw.extend_from_slice(b" HTTP/1.1\r\n\r\n");
        assert!(raw.len() > MAX_LINE_BYTES);
        let mut reader = std::io::Cursor::new(raw);
        match read_request(&mut reader).expect("read") {
            ReadOutcome::Rejected(response) => assert_eq!(response.status, 431),
            other => panic!("expected 431, got {other:?}"),
        }
    }

    #[test]
    fn one_hundred_and_one_headers_are_refused_with_431() {
        use std::fmt::Write as _;
        let mut raw = String::from("GET / HTTP/1.1\r\n");
        for index in 0..=MAX_HEADERS {
            let _ = write!(raw, "X-H{index}: v\r\n");
        }
        raw.push_str("\r\n");
        let mut reader = std::io::Cursor::new(raw.into_bytes());
        match read_request(&mut reader).expect("read") {
            ReadOutcome::Rejected(response) => assert_eq!(response.status, 431),
            other => panic!("expected 431 for 101 headers, got {other:?}"),
        }
    }

    #[test]
    fn an_oversized_content_length_is_refused_with_413() {
        let raw = b"GET / HTTP/1.1\r\nContent-Length: 2000000\r\n\r\n";
        let mut reader = std::io::Cursor::new(raw.to_vec());
        match read_request(&mut reader).expect("read") {
            ReadOutcome::Rejected(response) => assert_eq!(response.status, 413),
            other => panic!("expected 413, got {other:?}"),
        }
    }

    #[test]
    fn a_non_get_method_is_marked_unsupported() {
        let raw = b"POST /x HTTP/1.1\r\n\r\n";
        let mut reader = std::io::Cursor::new(raw.to_vec());
        let ReadOutcome::Ok(request) = read_request(&mut reader).expect("read") else {
            panic!("expected a request");
        };
        assert_eq!(request.path, "/__method");
    }

    #[test]
    fn a_response_carries_its_length_and_type() {
        let response = Response::ok("text/html; charset=utf-8", b"<p>hi</p>".to_vec());
        assert_eq!(response.status, 200);
        assert_eq!(response.body.len(), 9);
        assert_eq!(response.reason(), "OK");
        assert_eq!(Response::not_found("x").status, 404);
        assert_eq!(Response::bad_request("x").status, 400);
        assert_eq!(Response::payload_too_large().status, 413);
        assert_eq!(Response::header_fields_too_large().status, 431);
        assert_eq!(Response::misdirected().status, 421);
        assert_eq!(Response::misdirected().reason(), "Misdirected Request");
    }

    #[test]
    fn the_host_header_is_read() {
        let raw = b"GET / HTTP/1.1\r\nhOsT:  localhost:8181 \r\n\r\n";
        let mut reader = std::io::Cursor::new(raw.to_vec());
        let ReadOutcome::Ok(request) = read_request(&mut reader).expect("read") else {
            panic!("expected a request");
        };
        assert_eq!(request.host.as_deref(), Some("localhost:8181"));
    }

    #[test]
    fn only_loopback_names_on_this_port_are_allowed() {
        for host in [
            "127.0.0.1:8181",
            "localhost:8181",
            "LOCALHOST:8181",
            "[::1]:8181",
            "127.0.0.1",
            "localhost",
            "[::1]",
        ] {
            assert!(host_allowed(Some(host), 8181), "{host} should be allowed");
        }
        for host in [
            "evil.example:8181",
            "evil.example",
            "127.0.0.1:8182",
            "localhost:80",
            "127.0.0.1.evil.example:8181",
            "localhost.evil.example",
            "",
        ] {
            assert!(!host_allowed(Some(host), 8181), "{host} should be refused");
        }
        assert!(!host_allowed(None, 8181), "a missing Host is refused");
    }

    #[test]
    fn a_rebound_host_is_answered_421_without_reaching_the_handler() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let stop = Arc::new(AtomicBool::new(false));
        let stop_server = Arc::clone(&stop);
        let reached = Arc::new(AtomicBool::new(false));
        let reached_server = Arc::clone(&reached);
        let server = thread::spawn(move || {
            serve(&listener, &stop_server, move |_path, _query| {
                reached_server.store(true, Ordering::Relaxed);
                Response::ok("text/plain; charset=utf-8", b"ok".to_vec())
            })
        });
        let exchange = |request: String| -> String {
            let mut stream = TcpStream::connect(addr).expect("connect");
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("timeout");
            stream.write_all(request.as_bytes()).expect("write");
            let mut body = Vec::new();
            stream.read_to_end(&mut body).expect("read response");
            String::from_utf8_lossy(&body).into_owned()
        };

        let rebound = exchange(format!(
            "GET /api/corpora HTTP/1.1\r\nHost: evil.example:{}\r\n\r\n",
            addr.port()
        ));
        assert!(rebound.starts_with("HTTP/1.1 421"), "{rebound}");
        assert!(!reached.load(Ordering::Relaxed), "the handler must not run");

        let missing = exchange("GET /api/corpora HTTP/1.1\r\n\r\n".to_owned());
        assert!(missing.starts_with("HTTP/1.1 421"), "{missing}");

        let local = exchange(format!(
            "GET / HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n",
            addr.port()
        ));
        assert!(local.starts_with("HTTP/1.1 200"), "{local}");
        assert!(reached.load(Ordering::Relaxed));

        stop.store(true, Ordering::Relaxed);
        server.join().expect("server").expect("serve");
    }

    #[test]
    fn an_idle_connection_closes_and_the_server_serves_the_next() {
        // AUD-16: a peer that never sends data must not occupy the single
        // thread past the read timeout; the next client must still be served.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let stop = Arc::new(AtomicBool::new(false));
        let stop_server = Arc::clone(&stop);
        let server = thread::spawn(move || {
            serve(&listener, &stop_server, |_path, _query| {
                Response::ok("text/plain; charset=utf-8", b"ok".to_vec())
            })
        });

        let idle = TcpStream::connect(addr).expect("connect idle");
        idle.set_read_timeout(Some(IO_TIMEOUT + Duration::from_secs(2)))
            .expect("client read timeout");
        let started = Instant::now();
        let mut idle = idle;
        let mut buf = [0u8; 8];
        let read = idle.read(&mut buf);
        let elapsed = started.elapsed();
        let min_wait = IO_TIMEOUT
            .checked_sub(Duration::from_secs(1))
            .expect("timeout exceeds slack");
        assert!(
            elapsed >= min_wait,
            "idle peer must wait for the read timeout (~10 s), took {elapsed:?}"
        );
        assert!(
            elapsed <= IO_TIMEOUT + Duration::from_secs(1),
            "idle peer must be dropped within ~11 s, took {elapsed:?}"
        );
        // Timed-out server read closes the socket; the client sees EOF or a
        // connection error, not a hung read past the budget.
        assert!(
            matches!(read, Ok(0) | Err(_)),
            "idle connection must end, got {read:?}"
        );

        let mut live = TcpStream::connect(addr).expect("connect live");
        live.set_read_timeout(Some(Duration::from_secs(2)))
            .expect("live timeout");
        live.write_all(b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
            .expect("write");
        let mut body = Vec::new();
        live.read_to_end(&mut body).expect("read response");
        let text = String::from_utf8_lossy(&body);
        assert!(
            text.contains("200") && text.contains("ok"),
            "next request must be served after an idle drop: {text}"
        );

        stop.store(true, Ordering::Relaxed);
        server.join().expect("server").expect("serve");
    }
}
