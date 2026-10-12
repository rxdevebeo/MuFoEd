#!/usr/bin/env python3
"""Exercise an OCR-enabled CLI against a local mock Ollama endpoint (no model)."""
import argparse
import base64
import json
import subprocess
import threading
import zipfile
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--cli", required=True)
    parser.add_argument("--input", required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    seen = []
    recovered = "Recognized text from a mock vision provider."

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            seen.append((self.path, body))
            answer = {"details": {"digest": "sha256:mock-djvu"}} if self.path == "/api/show" else {"message": {"content": recovered}}
            payload = json.dumps(answer).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    args.out.parent.mkdir(parents=True, exist_ok=True)
    try:
        proc = subprocess.run([args.cli, "from-djvu", args.input, "--pages", "1", "--out", str(args.out), "--ocr", "--ocr-model", "mock-vision", "--ocr-endpoint", f"http://127.0.0.1:{server.server_port}"], capture_output=True, timeout=120)
    finally:
        server.shutdown()
        server.server_close()
    assert proc.returncode == 1, (proc.returncode, proc.stderr.decode(errors="replace"))
    assert [path for path, _ in seen] == ["/api/show", "/api/chat"], seen
    request = seen[1][1]
    assert request["model"] == "mock-vision" and request["stream"] is False
    image = base64.b64decode(request["messages"][0]["images"][0])
    assert image.startswith(b"\x89PNG\r\n\x1a\n")
    with zipfile.ZipFile(args.out) as package:
        xml = package.read("word/document.xml").decode()
        assert recovered in xml and "Recovered" in xml
        assert any(name.startswith("word/media/") for name in package.namelist())
    report = {"input": Path(args.input).name, "page": 1, "cli_exit": proc.returncode,
              "requests": [path for path, _ in seen], "png_bytes": len(image),
              "recovered_text_in_docx": True, "scan_in_docx": True,
              "provider": "local HTTP mock; no real OCR model quality assertion"}
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report))


if __name__ == "__main__":
    main()
