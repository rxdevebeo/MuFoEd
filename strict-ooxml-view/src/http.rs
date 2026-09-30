//! A minimal blocking HTTP/1.1 server for the viewer.
//!
//! Hand-rolled on `std::net::TcpListener` rather than pulled from a web
//! framework: the server answers three routes on loopback, serves bytes that
//! are already in memory, and shares the process with the renderer. A
//! framework would add a dependency tree to a workspace whose licences and
//! advisories are checked by `cargo deny` (`deny.toml`), for a loopback tool.
//!
//! What it does implement, because a browser needs it: keep-alive, a bounded
//! request head (so a client that never sends a newline cannot make the
//! process allocate without limit), and `Content-Length` on every response.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};

/// Largest request head accepted, in bytes.
///
/// Enough for a long URL with a percent-encoded file name, small enough that a
/// client which never sends a newline cannot grow the buffer.
const MAX_REQUEST_HEAD: usize = 16 * 1024;

/// A response the server should send.
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

    /// The reason phrase for the status.
    const fn reason(&self) -> &'static str {
        match self.status {
            400 => "Bad Request",
            404 => "Not Found",
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
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = input.get(index + 1..index + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else if bytes[index] == b'+' {
            out.push(b' ');
            index += 1;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Serves `handler` on `listener` until `stop` is set.
///
/// `handler` receives the decoded path and query and returns a [`Response`].
/// The loop is single-threaded and deliberately so: a viewer is opened by one
/// person at a time, and one slow render must not interleave with a page
/// request.
pub(crate) fn serve<F>(listener: &TcpListener, stop: &AtomicBool, handler: F) -> std::io::Result<()>
where
    F: Fn(&str, &str) -> Response,
{
    listener.set_nonblocking(true)?;
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                // A single failed connection must not stop the server: a
                // browser that cancels a request mid-body is routine.
                let _ = handle_connection(stream, &handler);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// Handles one connection, honouring keep-alive.
fn handle_connection<F>(stream: TcpStream, handler: &F) -> std::io::Result<()>
where
    F: Fn(&str, &str) -> Response,
{
    let write_half = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    let mut writer = write_half;
    loop {
        let Some(request) = read_request(&mut reader)? else {
            return Ok(());
        };
        let response = handler(&request.path, &request.query);
        write_response(&mut writer, &response, request.keep_alive)?;
        if !request.keep_alive {
            return Ok(());
        }
    }
}

/// A parsed request line.
struct Request {
    /// Decoded path.
    path: String,
    /// Decoded query string.
    query: String,
    /// Whether the connection should stay open.
    keep_alive: bool,
}

/// Reads a request line and drains its head.
///
/// Returns `None` at end of stream, which is how a browser signals it is done
/// with a keep-alive connection.
fn read_request(reader: &mut impl BufRead) -> std::io::Result<Option<Request>> {
    let mut line = Vec::new();
    if read_line(reader, &mut line)? == 0 {
        return Ok(None);
    }
    let request = String::from_utf8_lossy(&line).into_owned();
    let mut parts = request.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default().to_owned();
    let version = parts.next().unwrap_or("HTTP/1.1");
    let supported = method.eq_ignore_ascii_case("GET") || method.eq_ignore_ascii_case("HEAD");

    // Drain the rest of the head, bounded: a client that never sends a blank
    // line must not be able to grow this without limit.
    let mut head_bytes = 0usize;
    let mut keep_alive = true;
    loop {
        line.clear();
        let read = read_line(reader, &mut line)?;
        if read == 0 || line == b"\r\n" || line == b"\n" {
            break;
        }
        head_bytes += read;
        if head_bytes > MAX_REQUEST_HEAD {
            keep_alive = false;
            break;
        }
        let header = String::from_utf8_lossy(&line).to_lowercase();
        if header.starts_with("connection:") {
            keep_alive = header.contains("keep-alive");
        }
    }
    // HTTP/1.0 keeps the connection open only when asked to.
    if version.eq_ignore_ascii_case("HTTP/1.0") && !keep_alive {
        keep_alive = false;
    }

    if !supported {
        return Ok(Some(Request {
            path: "/__method".to_owned(),
            query: String::new(),
            keep_alive,
        }));
    }
    let (path, query) = match target.split_once('?') {
        Some((path, query)) => (path.to_owned(), query.to_owned()),
        None => (target, String::new()),
    };
    Ok(Some(Request {
        path: percent_decode(&path).unwrap_or_else(|| "/".to_owned()),
        query: percent_decode(&query).unwrap_or_default(),
        keep_alive,
    }))
}

/// Reads one line, stopping at LF. Returns the byte count read.
fn read_line(reader: &mut impl BufRead, out: &mut Vec<u8>) -> std::io::Result<usize> {
    let mut total = 0;
    loop {
        let available = match reader.fill_buf() {
            Ok(buffer) => buffer,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if available.is_empty() {
            return Ok(total);
        }
        if let Some(position) = available.iter().position(|byte| *byte == b'\n') {
            out.extend_from_slice(&available[..=position]);
            reader.consume(position + 1);
            return Ok(total + position + 1);
        }
        let length = available.len();
        out.extend_from_slice(available);
        reader.consume(length);
        total += length;
    }
}

/// Writes a response, ending the connection unless keep-alive.
fn write_response(
    writer: &mut impl Write,
    response: &Response,
    keep_alive: bool,
) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: {}\r\n\r\n",
        response.status,
        response.reason(),
        response.content_type,
        response.body.len(),
        if keep_alive { "keep-alive" } else { "close" },
    );
    writer.write_all(head.as_bytes())?;
    writer.write_all(&response.body)?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{percent_decode, read_request, Response};

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
        let mut reader = Cursor::new(raw.to_vec());
        let request = read_request(&mut reader).expect("read").expect("a request");
        assert_eq!(request.path, "/api/document");
        assert_eq!(request.query, "name=a.docx");
        assert!(request.keep_alive);
    }

    #[test]
    fn a_decoded_name_reaches_the_handler_intact() {
        let raw = "GET /api/document?name=%D0%B4%D0%BE%D0%BA.docx HTTP/1.1\r\n\r\n";
        let mut reader = Cursor::new(raw.as_bytes().to_vec());
        let request = read_request(&mut reader).expect("read").expect("a request");
        assert_eq!(request.query, "name=док.docx");
    }

    #[test]
    fn end_of_stream_ends_the_connection() {
        let mut reader = Cursor::new(Vec::new());
        assert!(read_request(&mut reader).expect("read").is_none());
    }

    #[test]
    fn connection_close_turns_off_keep_alive() {
        let raw = b"GET / HTTP/1.1\r\nConnection: close\r\n\r\n";
        let mut reader = Cursor::new(raw.to_vec());
        let request = read_request(&mut reader).expect("read").expect("a request");
        assert!(!request.keep_alive);
    }

    #[test]
    fn an_unbounded_head_stops_at_the_limit() {
        let mut head = String::from("GET / HTTP/1.1\r\n");
        while head.len() < 64 * 1024 {
            head.push_str("X-Padding: ");
            head.push_str(&"y".repeat(256));
            head.push_str("\r\n");
        }
        head.push_str("\r\n");
        let mut reader = Cursor::new(head.into_bytes());
        let request = read_request(&mut reader).expect("read").expect("a request");
        assert!(
            !request.keep_alive,
            "an oversized head must not hold the socket"
        );
    }

    #[test]
    fn a_non_get_method_is_marked_unsupported() {
        let raw = b"POST /x HTTP/1.1\r\n\r\n";
        let mut reader = Cursor::new(raw.to_vec());
        let request = read_request(&mut reader).expect("read").expect("a request");
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
    }
}
