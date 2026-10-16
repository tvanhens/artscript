#!/usr/bin/env python3
"""Live artscript preview: type a prompt, watch the drawing render while the model streams it.

usage: serve.py [--port 8765] [--project examples/live] [--bin target/release/artscript]
                [--replay scripts/live/replays/fox.json] [--model claude-sonnet-4-5]

With an Anthropic key in ANTHROPIC_API_KEY (or ANTHROPIC_KEY) the prompt goes to the model.
With --replay, a recorded response streams at --tokens-per-sec instead, and the page says so.
"""

import argparse
import base64
import json
import os
import queue
import subprocess
import tempfile
import threading
import time
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
ENTRY = "art/live.art"

def system_prompt(project: Path) -> str:
    card = (REPO / "docs/language.md").read_text()
    palettes = "\n".join(p.read_text() for p in sorted((project / "lib/palette").glob("*.art")))
    return (
        f"{card}\n## Default palette `p`\n\n```\n{palettes}```\n\n"
        "The user watches the file render after every line. Start with `@target live.png 512x512`. "
        "Reply with one ```art fenced block."
    )


def source_of(text: str) -> str:
    """The artscript inside a partial model reply: the first fenced block, or from `@target` on."""
    if "```" in text:
        body = text.split("```", 1)[1]
        body = body.split("\n", 1)[1] if "\n" in body else ""
        return body.split("```", 1)[0]
    at = text.find("@target")
    return text[at:] if at >= 0 else ""


def complete_lines(src: str) -> str:
    return src[: src.rfind("\n") + 1]


class Renderer:
    def __init__(self, binary: Path, project: Path):
        self.binary = binary
        self.project = project
        self.out = Path(tempfile.mkdtemp(prefix="artscript-live-")) / "frame.png"

    def render(self, src: str) -> dict:
        self.out.unlink(missing_ok=True)
        start = time.perf_counter()
        proc = subprocess.run(
            [str(self.binary), "render", ENTRY, "--stdin", "-o", str(self.out), "--json"],
            cwd=self.project, input=src.encode(), capture_output=True,
        )
        ms = (time.perf_counter() - start) * 1000
        try:
            diags = json.loads(proc.stdout or b"{}").get("diagnostics", [])
        except json.JSONDecodeError:
            diags = []
        png = base64.b64encode(self.out.read_bytes()).decode() if self.out.exists() else None
        return {"png": png, "diagnostics": diags, "exit": proc.returncode, "ms": round(ms)}


def anthropic_chunks(prompt: str, system: str, model: str, key: str):
    body = json.dumps({
        "model": model, "max_tokens": 8000, "stream": True, "system": system,
        "messages": [{"role": "user", "content": prompt}],
    }).encode()
    req = urllib.request.Request("https://api.anthropic.com/v1/messages", data=body, headers={
        "x-api-key": key, "anthropic-version": "2023-06-01", "content-type": "application/json",
    })
    with urllib.request.urlopen(req) as resp:
        for raw in resp:
            line = raw.decode().strip()
            if not line.startswith("data:"):
                continue
            ev = json.loads(line[5:])
            if ev.get("type") == "content_block_delta" and ev["delta"].get("type") == "text_delta":
                yield ev["delta"]["text"]


def replay_chunks(path: Path, tokens_per_sec: float):
    """Streams a recorded reply in token-sized pieces, about four characters each."""
    text = json.loads(path.read_text())["response"]
    i = 0
    while i < len(text):
        n = 4 if text[i].isalnum() else 1
        while i + n < len(text) and text[i + n - 1].isalnum() and text[i + n].isalnum() and n < 6:
            n += 1
        yield text[i : i + n]
        i += n
        time.sleep(1 / tokens_per_sec)


class Generation:
    """One prompt's run: the model stream feeds text, a render loop draws the newest complete prefix."""

    def __init__(self, chunks, renderer: Renderer):
        self.events: queue.Queue = queue.Queue()
        self.renderer = renderer
        self.chunks = chunks
        self.text = ""
        self.wanted = ""
        self.done = False
        self.cond = threading.Condition()
        self.started = time.perf_counter()
        self.first_frame = None

    def run(self):
        threading.Thread(target=self.render_loop, daemon=True).start()
        try:
            for chunk in self.chunks:
                self.text += chunk
                self.events.put(("token", {"text": chunk}))
                self.want(complete_lines(source_of(self.text)))
        except Exception as e:
            self.events.put(("failure", {"message": str(e)}))
        with self.cond:
            self.wanted = source_of(self.text)
            self.done = True
            self.cond.notify()

    def want(self, src: str):
        with self.cond:
            if src.strip() and src != self.wanted:
                self.wanted = src
                self.cond.notify()

    def render_loop(self):
        drawn = None
        while True:
            with self.cond:
                while self.wanted == drawn and not self.done:
                    self.cond.wait()
                src, final = self.wanted, self.done and self.wanted == drawn
            if final:
                break
            frame = self.renderer.render(src)
            drawn = src
            elapsed = round((time.perf_counter() - self.started) * 1000)
            if frame["png"] and self.first_frame is None:
                self.first_frame = elapsed
            frame.update(lines=src.count("\n"), source=src, elapsed=elapsed, first_frame=self.first_frame)
            self.events.put(("frame", frame))
        self.events.put(("done", {"elapsed": round((time.perf_counter() - self.started) * 1000)}))


class Handler(BaseHTTPRequestHandler):
    server_version = "artscript-live"

    def log_message(self, *args):
        pass

    def do_GET(self):
        if self.path == "/":
            page = (HERE / "index.html").read_bytes()
            self.send_response(200)
            self.send_header("content-type", "text/html; charset=utf-8")
            self.end_headers()
            self.wfile.write(page)
        elif self.path == "/info":
            self.json({"provider": self.server.provider, "model": self.server.model_label})
        else:
            self.send_error(404)

    def json(self, obj):
        data = json.dumps(obj).encode()
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.end_headers()
        self.wfile.write(data)

    def do_POST(self):
        if self.path != "/generate":
            return self.send_error(404)
        prompt = json.loads(self.rfile.read(int(self.headers["content-length"])))["prompt"]
        srv = self.server
        if srv.provider == "replay":
            chunks = replay_chunks(srv.replay, srv.tokens_per_sec)
        else:
            chunks = anthropic_chunks(prompt, srv.system, srv.model, srv.key)
        gen = Generation(chunks, srv.renderer)
        threading.Thread(target=gen.run, daemon=True).start()
        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.send_header("cache-control", "no-cache")
        self.end_headers()
        while True:
            kind, data = gen.events.get()
            self.wfile.write(f"event: {kind}\ndata: {json.dumps(data)}\n\n".encode())
            self.wfile.flush()
            if kind == "done":
                break


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=8765)
    ap.add_argument("--project", type=Path, default=REPO / "examples/live")
    ap.add_argument("--bin", type=Path, default=REPO / "target/release/artscript")
    ap.add_argument("--replay", type=Path)
    ap.add_argument("--tokens-per-sec", type=float, default=70)
    ap.add_argument("--model", default="claude-sonnet-4-5")
    args = ap.parse_args()

    srv = ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
    srv.renderer = Renderer(args.bin.resolve(), args.project.resolve())
    srv.system = system_prompt(args.project)
    srv.key = os.environ.get("ANTHROPIC_API_KEY") or os.environ.get("ANTHROPIC_KEY")
    srv.model = args.model
    srv.replay = args.replay
    srv.tokens_per_sec = args.tokens_per_sec
    if args.replay:
        srv.provider, srv.model_label = "replay", f"recorded reply, {args.tokens_per_sec:g} tokens/s"
    elif srv.key:
        srv.provider, srv.model_label = "anthropic", args.model
    else:
        raise SystemExit("set ANTHROPIC_API_KEY, or pass --replay FILE")
    print(f"artscript live on http://127.0.0.1:{args.port} ({srv.model_label})")
    srv.serve_forever()


if __name__ == "__main__":
    main()
