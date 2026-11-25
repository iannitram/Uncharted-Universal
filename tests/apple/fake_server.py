#!/usr/bin/env python3
"""A local fake server for apple/tests/relay_host_test.sh. Writes its port to the file named in argv[1].

  GET  /ok          200 "hello"
  GET  /big         200 with 5000 bytes
  GET  /missing     404 "gone"
  POST /echo        200, a JSON object: the request's Content-Type, Accept-Encoding, X-Test header and body
  GET  /requests    200, a JSON list of "METHOD path" for every request seen so far
  anything else     404
"""
import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

SEEN = []


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def reply(self, status, body, content_type="text/plain", extra=None):
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        for name, value in (extra or {}).items():
            self.send_header(name, value)
        self.end_headers()
        self.wfile.write(body)

    def handle_any(self):
        length = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(length) if length else b""
        if self.path != "/requests":
            SEEN.append(f"{self.command} {self.path}")
        if self.command == "GET" and self.path == "/ok":
            self.reply(200, b"hello", extra={"X-Answer": "yes"})
        elif self.command == "GET" and self.path == "/big":
            self.reply(200, b"x" * 5000)
        elif self.command == "GET" and self.path == "/missing":
            self.reply(404, b"gone")
        elif self.command == "POST" and self.path == "/echo":
            seen = {
                "content_type": self.headers.get("Content-Type"),
                "accept_encoding": self.headers.get("Accept-Encoding"),
                "x_test": self.headers.get("X-Test"),
                "body": body.decode("utf-8", "replace"),
            }
            self.reply(200, json.dumps(seen).encode(), "application/json")
        elif self.command == "GET" and self.path == "/requests":
            self.reply(200, json.dumps(SEEN).encode(), "application/json")
        else:
            self.reply(404, b"no")

    do_GET = do_POST = do_PUT = do_DELETE = do_PROPFIND = do_MKCOL = handle_any


server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
with open(sys.argv[1], "w") as out:
    out.write(str(server.server_address[1]))
server.serve_forever()
