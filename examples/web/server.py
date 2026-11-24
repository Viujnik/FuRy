"""Read-only FuRy demo on the loopback interface."""

import argparse
import asyncio
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, HTTPServer
import os
from pathlib import Path

from fury import FuryDB
from models import Item


PAGE_SIZE = 3
QUERY = """
SELECT id, title, available, note
FROM (VALUES
    (9007199254740993::bigint, 'Café beans', true, NULL::text),
    (9007199254740994::bigint, 'Oat milk', true, 'Chilled'),
    (9007199254740995::bigint, 'Chocolate', false, 'Restocking')
) AS items(id, title, available, note)
ORDER BY id
LIMIT $1::bigint
"""
ASSETS = {
    "/": ("index.html", "text/html; charset=utf-8"),
    "/app.js": ("app.js", "text/javascript"),
    "/records.js": ("records.js", "text/javascript"),
    "/models/item.js": ("models/item.js", "text/javascript"),
    "/registry.bin": ("registry.bin", "application/octet-stream"),
    "/wasm/fury_ts.js": ("wasm/fury_ts.js", "text/javascript"),
    "/wasm/fury_ts_bg.wasm": ("wasm/fury_ts_bg.wasm", "application/wasm"),
}


async def fetch_frames(database):
    items = await database.fetch_all(Item, QUERY, [PAGE_SIZE])
    return b"".join(item.to_bytes() for item in items)


def create_server(site, port):
    database = FuryDB.connect(os.environ["FURY_TEST_DATABASE_URL"])
    assets = {
        url: ((site / filename).read_bytes(), media_type)
        for url, (filename, media_type) in ASSETS.items()
    }

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            if self.path == "/api/items":
                try:
                    data = asyncio.run(fetch_frames(database))
                except (RuntimeError, TypeError, ValueError):
                    self.send_error(HTTPStatus.INTERNAL_SERVER_ERROR, "Could not load demo records")
                    return
                media_type = "application/octet-stream"
            elif self.path in assets:
                data, media_type = assets[self.path]
            else:
                self.send_error(HTTPStatus.NOT_FOUND)
                return

            self.send_response(HTTPStatus.OK)
            self.send_header("Content-Type", media_type)
            self.send_header("Content-Length", str(len(data)))
            self.send_header("X-Content-Type-Options", "nosniff")
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.wfile.write(data)

    return HTTPServer(("127.0.0.1", port), Handler)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("site", type=Path)
    parser.add_argument("--port", type=int, default=8000)
    args = parser.parse_args()
    with create_server(args.site, args.port) as server:
        print(f"Open http://127.0.0.1:{server.server_port}", flush=True)
        try:
            server.serve_forever()
        except KeyboardInterrupt:
            pass


if __name__ == "__main__":
    main()
