"""Exercise the deployed HTTP routes with the same WASM code as the browser."""

import argparse
from pathlib import Path
import subprocess
from threading import Thread
from urllib.request import urlopen

from server import ASSETS, create_server


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("site", type=Path)
    parser.add_argument("--browser", help="Path to a Chromium or Google Chrome executable")
    args = parser.parse_args()

    with create_server(args.site, 0) as server:
        worker = Thread(target=server.serve_forever)
        worker.start()
        origin = f"http://127.0.0.1:{server.server_port}"
        try:
            for path, (_, media_type) in ASSETS.items():
                with urlopen(origin + path, timeout=5) as response:
                    assert response.headers["Content-Type"] == media_type
                    assert response.read()
            subprocess.run(["node", args.site / "smoke.mjs", origin], check=True, timeout=20)
            if args.browser:
                page = subprocess.run(
                    [
                        args.browser, "--headless", "--no-first-run",
                        f"--user-data-dir={args.site.parent / 'chrome-profile'}",
                        "--virtual-time-budget=8000", "--dump-dom", origin,
                    ],
                    check=True, capture_output=True, text=True, timeout=45,
                ).stdout
                assert 'data-state="ready"' in page, page
                assert "9007199254740993" in page and "Café beans" in page, page
                print("Headless browser rendered the expected records")
        finally:
            server.shutdown()
            worker.join()

    print("Web demo passed (PostgreSQL → HTTP → generated TypeScript/WASM)")


if __name__ == "__main__":
    main()
