#!/usr/bin/env python3
"""Serveur local de beacons pour les essais de progression.

Lance-le puis démarre l'app avec :

    YOUTUS_TRACKING_ORIGIN=http://127.0.0.1:8765 youtus ...

Chaque POST sur /playback, /atr ou /watchtime est journalisé sur stdout, avec les
paramètres utiles (cpn, et, cmt, state, final). Réponse 204, comme YouTube.
"""

import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlsplit

KEEP = ("cpn", "ver", "cplayer", "st", "et", "cmt", "state", "final")


class Handler(BaseHTTPRequestHandler):
    def do_POST(self) -> None:
        length = int(self.headers.get("Content-Length") or 0)
        if length:
            self.rfile.read(length)
        parts = urlsplit(self.path)
        query = parse_qs(parts.query, keep_blank_values=True)
        values = {name: query[name][0] for name in KEEP if name in query}
        print(f"POST {parts.path} {values}", flush=True)
        self.send_response(204)
        self.end_headers()

    def log_message(self, *args) -> None:
        pass


def main() -> None:
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8765
    print(f"beacon server on http://127.0.0.1:{port}", flush=True)
    ThreadingHTTPServer(("127.0.0.1", port), Handler).serve_forever()


if __name__ == "__main__":
    main()
