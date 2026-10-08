"""Local crawler -> explorer contract smoke with synthetic HTTP fixtures.

Usage: python scripts/bridge_smoke.py ../aeo-crawler
Fixture traffic stays on loopback. Go and Cargo may fetch missing dependencies.
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.error import URLError
from urllib.request import ProxyHandler, Request, build_opener


def serve(document: dict) -> ThreadingHTTPServer:
    class Handler(BaseHTTPRequestHandler):
        def do_GET(self) -> None:
            if self.path != "/.well-known/aeo.json":
                self.send_error(404)
                return
            body = json.dumps(document).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, _format: str, *_args: object) -> None:
            pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


def free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: python scripts/bridge_smoke.py PATH_TO_AEO_CRAWLER", file=sys.stderr)
        return 2
    crawler_dir = Path(sys.argv[1]).resolve()
    graph_dir = Path(__file__).resolve().parents[1]
    if not (crawler_dir / "go.mod").is_file():
        print("crawler path must contain go.mod", file=sys.stderr)
        return 2

    child = {
        "aeo_version": "0.1",
        "entity": {
            "id": "https://child.example/#org",
            "type": "Organization",
            "name": "Child",
            "canonical_url": "https://child.example/",
        },
        "authority": {"primary_sources": []},
        "claims": [{"id": "c2", "predicate": "industry", "value": "testing"}],
    }
    child_server = serve(child)
    parent = {
        "aeo_version": "0.1",
        "entity": {
            "id": "https://parent.example/#org",
            "type": "Organization",
            "name": "Parent",
            "canonical_url": "https://parent.example/",
        },
        "authority": {
            "primary_sources": [
                f"http://127.0.0.1:{child_server.server_port}/evidence"
            ]
        },
        "claims": [{"id": "c1", "predicate": "industry", "value": "testing"}],
    }
    parent_server = serve(parent)
    graph_process: subprocess.Popen[bytes] | None = None
    try:
        crawl = subprocess.run(
            [
                "go",
                "run",
                "./cmd/aeo-crawler",
                "--seed",
                f"http://127.0.0.1:{parent_server.server_port}",
                "--depth",
                "1",
                "--format",
                "graph",
            ],
            cwd=crawler_dir,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=True,
            timeout=90,
        )
        lines = [json.loads(line) for line in crawl.stdout.splitlines()]
        assert len(lines) == 2, (
            f"expected 2 crawler nodes, got {len(lines)}; "
            f"crawler stderr={crawl.stderr.decode(errors='replace')}; "
            f"origins={[line.get('provenance', {}).get('origin') for line in lines]}"
        )
        assert all("body" in line and "provenance" in line for line in lines)

        subprocess.run(["cargo", "build", "--locked"], cwd=graph_dir, check=True, timeout=600)
        binary = graph_dir / "target" / "debug" / (
            "aeo-graph-explorer.exe" if os.name == "nt" else "aeo-graph-explorer"
        )
        port = free_port()
        token = secrets.token_hex(32)
        env = os.environ.copy()
        env.update({"HOST": "127.0.0.1", "PORT": str(port), "AEO_GRAPH_INGEST_TOKEN": token})
        env.pop("AUDIT_STREAM_URL", None)
        graph_process = subprocess.Popen(
            [str(binary)], cwd=graph_dir, env=env,
            stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
        )
        opener = build_opener(ProxyHandler({}))
        base = f"http://127.0.0.1:{port}"
        for _ in range(100):
            if graph_process.poll() is not None:
                raise RuntimeError("graph service exited before healthz")
            try:
                with opener.open(f"{base}/healthz", timeout=0.5) as response:
                    if response.status == 200:
                        break
            except URLError:
                time.sleep(0.1)
        else:
            raise TimeoutError("graph service did not become healthy")

        request = Request(
            f"{base}/ingest",
            data=crawl.stdout,
            headers={"Authorization": f"Bearer {token}"},
            method="POST",
        )
        with opener.open(request, timeout=10) as response:
            ingested = json.load(response)
        assert ingested["nodes"] == 2 and ingested["edges"] == 1, ingested
        with opener.open(f"{base}/shortest-path?from=https%3A%2F%2Fparent.example%2F%23org&to=https%3A%2F%2Fchild.example%2F%23org", timeout=5) as response:
            path = json.load(response)
        assert path["found"] and path["length"] == 1, path
        with opener.open(f"{base}/find-by-claim?predicate=industry&value=testing", timeout=5) as response:
            claims = json.load(response)
        assert len(claims) == 2, claims
        print("bridge smoke passed: 2 fetched nodes, 1 authority edge, 1-hop path, 2 claim matches")
        return 0
    except Exception:
        if graph_process is not None and graph_process.poll() is not None:
            print(
                "graph process stderr: " + graph_process.stderr.read().decode(errors="replace"),
                file=sys.stderr,
            )
        raise
    finally:
        if graph_process is not None:
            graph_process.terminate()
            try:
                graph_process.communicate(timeout=5)
            except subprocess.TimeoutExpired:
                graph_process.kill()
                graph_process.communicate(timeout=5)
        parent_server.shutdown()
        parent_server.server_close()
        child_server.shutdown()
        child_server.server_close()


if __name__ == "__main__":
    raise SystemExit(main())
