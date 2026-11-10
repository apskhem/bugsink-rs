"""Records what alerts send, for tools/parity/digest.sh: HTTP POSTs (messaging webhooks) on :8025 and emails (a minimal
SMTP sink) on :2525. Each request becomes a JSON line in /out/capture.jsonl. Standard library only."""
import email
import email.policy
import json
import socketserver
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

OUT = "/out/capture.jsonl"
lock = threading.Lock()


def record(entry):
    with lock, open(OUT, "a") as f:
        f.write(json.dumps(entry, sort_keys=True) + "\n")


class Http(BaseHTTPRequestHandler):
    def do_POST(self):
        body = self.rfile.read(int(self.headers.get("Content-Length", 0))).decode()
        record({"kind": "http", "path": self.path, "content_type": self.headers.get("Content-Type"), "body": body})
        failing = self.path.startswith("/fail")  # to test failure bookkeeping
        self.send_response(500 if failing else 200)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(b'{"error": "failing on purpose"}' if failing else b'{"ok": true}')

    def log_message(self, *args):
        pass


class Smtp(socketserver.StreamRequestHandler):
    def reply(self, line):
        self.wfile.write(line.encode() + b"\r\n")

    def handle(self):
        self.reply("220 capture ESMTP")
        mail_from, rcpt_to = None, []
        while line := self.rfile.readline():
            command = line.decode().strip()
            verb = command.split(" ", 1)[0].upper()
            if verb in ("EHLO", "HELO"):
                self.reply("250-capture")
                self.reply("250 8BITMIME")
            elif verb == "MAIL":
                mail_from, rcpt_to = command[10:], []
                self.reply("250 OK")
            elif verb == "RCPT":
                rcpt_to.append(command[8:])
                self.reply("250 OK")
            elif verb == "DATA":
                self.reply("354 go ahead")
                data = b""
                while (chunk := self.rfile.readline()) not in (b".\r\n", b""):
                    data += chunk[1:] if chunk.startswith(b"..") else chunk
                record_email(mail_from, rcpt_to, data)
                self.reply("250 OK")
            elif verb == "QUIT":
                self.reply("221 bye")
                return
            else:  # RSET, NOOP, ...
                self.reply("250 OK")


def record_email(mail_from, rcpt_to, data):
    msg = email.message_from_bytes(data, policy=email.policy.default)
    record({
        "kind": "smtp",
        "envelope_from": mail_from,
        "envelope_to": rcpt_to,
        "from": str(msg["From"]),
        "to": str(msg["To"]),
        "subject": str(msg["Subject"]),
        "parts": [part.get_content_type() for part in msg.walk()],
        "text": msg.get_body(preferencelist=("plain",)).get_content(),
        "html": msg.get_body(preferencelist=("html",)).get_content(),
    })


class ThreadingTcp(socketserver.ThreadingMixIn, socketserver.TCPServer):
    allow_reuse_address = True
    daemon_threads = True


if __name__ == "__main__":
    threading.Thread(target=ThreadingTcp(("0.0.0.0", 2525), Smtp).serve_forever, daemon=True).start()
    ThreadingHTTPServer(("0.0.0.0", 8025), Http).serve_forever()
