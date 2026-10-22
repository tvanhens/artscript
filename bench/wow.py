# /// script
# requires-python = ">=3.11"
# dependencies = ["pillow"]
# ///
"""First-shot quality eval for the ramose plugin: N clean `/ramose <prompt>` runs, each scored 1-20 on six axes by two judges.

`wow.py` measures how good a first `/ramose <prompt>` result looks. It runs N clean `claude -p`
sessions that load only `plugins/ramose` and the ramose binary built from this checkout, so a
change to the skill, the docs, or the std library is what it measures. The judges score each image
within its style, the one the prompt names or the one the image chose, so no style wins for being
that style. Each PNG goes to two judges, opus and sonnet, which score it from 1 to 20 on fidelity,
composition, color, form, polish, and wow. A trial's composite is the mean of both judges over the
six axes. A third judge, fable, was dropped: it was the noisiest on a re-judge of the same images,
it agreed least with the other two, and as the middle scorer it set the median in 80 of 104
trials.

  uv run bench/wow.py --label base                  # 6 trials of the fox prompt
  uv run bench/wow.py --label base --prompt chest   # the treasure chest icon prompt
  uv run bench/wow.py --label base --prompt lighthouse  # a prompt that names a style
  uv run bench/wow.py --label ctl --judge-only a.png b.png

Results, PNGs, and the `.ramo` sources go to `bench/out/wow/<label>-<prompt>/`. One trial's
composite moves by about 0.5 from run to run, so compare means of at least 6 trials, and run all
three prompts before you trust a change.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
PROMPTS = {
    "fox": "draw a cute fox sitting next to a glowing mushroom",
    "chest": "make a game icon of a treasure chest overflowing with gold coins",
    "lighthouse": "paint a lighthouse on a cliff at dusk in a loose watercolor style",
}
JUDGES = ("claude-opus-5-5", "claude-sonnet-5-5")
AXES = ("fidelity", "composition", "color", "form", "polish", "wow")
TOOLS = "Bash(ramose *),Read,Write,Edit,Glob,Grep,Skill"
TRIAL_TIMEOUT_S = 20 * 60

RUBRIC = """You are an art director judging one image made by an AI agent from this request: "{prompt}"

Judge the image within its style: the style the request names, or the one the image chose when the request names none. No style scores higher than another for being that style, but an image that ignores a style the request names loses fidelity.

Score each axis from 1 to 20. Use the whole range. Anchors for every axis:
1-4 broken or unreadable. 5-8 generic programmer art: crude primitives, no intent. 9-12 a competent amateur asset.
13-16 polished, professional work you would ship. 17-20 portfolio-grade, striking, an artist would be proud of it.

- fidelity: does it clearly show what was asked, in the style asked for, with the subject readable at a glance?
- composition: framing, silhouette, balance, use of the canvas, nothing cut off by accident.
- color: palette harmony, value contrast, color as light (glow, shadow, atmosphere).
- form: believable construction, proportions, and depth, with volume and shading where the style uses them.
- polish: clean, deliberate edges and a consistent level of detail for its style, no artifacts, stray shapes, or misplaced parts.
- wow: would a person seeing this as their first result from the tool say "wow"?

Reply with only a JSON object: {{"fidelity":n,"composition":n,"color":n,"form":n,"polish":n,"wow":n,"notes":"two sentences: the biggest strength and the biggest flaw"}}"""


SCHEMA = {
    "type": "object",
    "properties": {a: {"type": "integer", "minimum": 1, "maximum": 20} for a in AXES} | {"notes": {"type": "string"}},
    "required": [*AXES, "notes"],
}


def env_for(bin_dir: Path | None = None) -> dict:
    # an API key in the environment overrides the logged-in CLI session
    env = {k: v for k, v in os.environ.items() if k != "ANTHROPIC_API_KEY"}
    if bin_dir:
        env["PATH"] = f"{bin_dir}{os.pathsep}{env['PATH']}"
    return env


def claude(prompt: str, cwd: Path, model: str, tools: str, extra: list[str], env: dict, timeout: int) -> dict:
    cmd = [
        "claude", "-p", "--model", model, "--output-format", "json", "--setting-sources", "project",
        "--strict-mcp-config", "--allowedTools", tools, "--permission-mode", "acceptEdits", "--no-session-persistence", *extra,
    ]
    try:
        r = subprocess.run(cmd, input=prompt, cwd=cwd, env=env, capture_output=True, text=True, timeout=timeout)
        return json.loads(r.stdout)
    except subprocess.TimeoutExpired:
        return {"is_error": True, "result": "timeout"}
    except json.JSONDecodeError:
        return {"is_error": True, "result": f"unparseable: {r.stdout[-300:]!r} {r.stderr[-300:]!r}"}


def cli_judge(model: str, prompt: str, shot: Path) -> dict:
    text = RUBRIC.format(prompt=prompt) + f"\n\nThe image is at {shot}. Read it, then answer."
    for _ in range(3):
        res = claude(text, shot.parent, model, "Read", ["--json-schema", json.dumps(SCHEMA)], env_for(), 600)
        out = res.get("structured_output") or {}
        if all(isinstance(out.get(a), int) for a in AXES):
            return out
    return {"error": str(res.get("result"))[:300]}


def flatten(path: Path, dest: Path) -> Path:
    img = Image.open(path).convert("RGBA")
    flat = Image.new("RGBA", img.size, (204, 204, 204, 255))
    flat.alpha_composite(img)
    if max(flat.size) > 1024:
        flat.thumbnail((1024, 1024))
    dest.mkdir(parents=True, exist_ok=True)
    shot = dest / "judge.png"
    flat.convert("RGB").save(shot)
    return shot


def judge_image(prompt: str, path: Path, dest: Path) -> dict:
    shot = flatten(path, dest)
    with ThreadPoolExecutor(len(JUDGES)) as ex:
        verdicts = dict(zip(JUDGES, ex.map(lambda m: cli_judge(m, prompt, shot), JUDGES)))
    ok = [v for v in verdicts.values() if "error" not in v]
    composite = statistics.mean(statistics.mean(v[a] for a in AXES) for v in ok) if ok else None
    axes = {a: statistics.mean(v[a] for v in ok) for a in AXES} if ok else {}
    return {"judges": verdicts, "composite": composite, "axes": axes, "judge_errors": len(JUDGES) - len(ok)}


def pick_png(project: Path) -> Path | None:
    pngs = [p for p in project.rglob("*.png") if ".ramose" not in p.parts and ".claude" not in p.parts]
    if not pngs:
        return None
    return max(pngs, key=lambda p: (Image.open(p).size[0] * Image.open(p).size[1], p.stat().st_mtime))


def trial(i: int, prompt: str, model: str, out: Path, bin_dir: Path) -> dict:
    with tempfile.TemporaryDirectory(prefix="wow-") as tmp:
        project = Path(tmp) / "project"
        project.mkdir()
        t0 = time.monotonic()
        res = claude(f"/ramose {prompt}", project, model, TOOLS, ["--plugin-dir", str(ROOT / "plugins/ramose")], env_for(bin_dir), TRIAL_TIMEOUT_S)
        dur = time.monotonic() - t0
        dest = out / f"trial{i}"
        for sub in ("art", "assets", "lib", "ramose.toml"):
            src = project / sub
            if src.is_dir():
                shutil.copytree(src, dest / sub)
            elif src.exists():
                dest.mkdir(parents=True, exist_ok=True)
                shutil.copy(src, dest / sub)
        img = pick_png(project)
        row = {
            "trial": i, "is_error": bool(res.get("is_error")), "turns": res.get("num_turns"),
            "cost_usd": res.get("total_cost_usd"), "duration_s": round(dur), "result": str(res.get("result", ""))[-600:],
            "image": None, "composite": None,
        }
        if img:
            shutil.copy(img, out / f"trial{i}.png")
            row["image"] = str(img.relative_to(project))
            row |= judge_image(prompt, out / f"trial{i}.png", out / f"judge{i}")
        return row


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--prompt", default="fox", choices=sorted(PROMPTS))
    ap.add_argument("--trials", type=int, default=6)
    ap.add_argument("--model", default="claude-opus-5-5")
    ap.add_argument("--label", required=True)
    ap.add_argument("--judge-only", nargs="*", help="score existing PNGs instead of running trials")
    args = ap.parse_args()
    out = ROOT / "bench/out/wow" / f"{args.label}-{args.prompt}"
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)
    prompt = PROMPTS[args.prompt]

    if args.judge_only:
        rows = [{"trial": i, "image": p} | judge_image(prompt, Path(p), out / f"judge{i}") for i, p in enumerate(args.judge_only)]
    else:
        bin_dir = ROOT / "target/release"
        subprocess.run(["cargo", "build", "--release", "-q", "-p", "ramose"], cwd=ROOT, check=True)
        with ThreadPoolExecutor(args.trials) as ex:
            rows = list(ex.map(lambda i: trial(i, prompt, args.model, out, bin_dir), range(args.trials)))

    scored = [r["composite"] for r in rows if r.get("composite") is not None]
    summary = {
        "label": args.label, "prompt": args.prompt, "model": args.model, "trials": len(rows),
        "scored": len(scored), "run_errors": sum(bool(r.get("is_error")) for r in rows),
        "no_image": sum(r.get("image") is None for r in rows),
        "judge_errors": sum(r.get("judge_errors", 0) for r in rows),
        "composite_mean": round(statistics.mean(scored), 2) if scored else None,
        "composites": [round(s, 2) for s in scored],
        "axes_mean": {a: round(statistics.mean(r["axes"][a] for r in rows if r.get("axes")), 2) for a in AXES} if scored else {},
        "cost_usd": round(sum(r.get("cost_usd") or 0 for r in rows), 2),
        "turns": [r.get("turns") for r in rows],
    }
    (out / "report.json").write_text(json.dumps({"summary": summary, "rows": rows}, indent=2))
    print(json.dumps(summary, indent=2))
    for r in rows:
        for m, v in (r.get("judges") or {}).items():
            print(f"  t{r['trial']} {m}: {v.get('notes', v.get('error'))}")
    return 0 if scored else 1


if __name__ == "__main__":
    sys.exit(main())
