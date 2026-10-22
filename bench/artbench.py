# /// script
# requires-python = ">=3.11"
# dependencies = ["pillow"]
# ///
"""Token benchmark and quality eval for the ramose skill: one create plus three edits through `claude -p`.

artbench checks that the ramose skill in `plugins/ramose/skills/ramose` still makes good art, and
measures what it costs. It runs Claude Code (`claude -p`) through one fixed scenario in a scratch
project: draw a chibi mushroom, then recolor the cap, add a background, and add an idle GIF. Each
step is a new session.

For each step, artbench records:

- Deterministic checks: `check`, `build`, and `fmt --check` pass, the outputs exist at the right
  size, and pixel tests confirm the requested change.
- A judge score from 1 to 10, given by a separate `claude -p` call that looks at the PNG.
- Tokens and cost, split into input, cache write, cache read, and output.

A run fails when a check fails, when a check that passed in `baseline.json` now fails, or when a
judge score drops more than 2 below the baseline. A step that costs more than 1.5 times its
baseline gets a warning.

Run it on a pull request

Comment `@artbench` on the pull request. To use a different model, add it, for example `@artbench
model=claude-sonnet-5-5`. Only owners, members, and collaborators can start a run. The workflow
posts the summary as a comment and uploads the art and `report.json` as an artifact.

GitHub runs a workflow for a plain PR comment only from the default branch. Until `artbench.yml`
is on `main`, start a run with a review comment instead. The review events use the pull request's
copy of the workflow.

Run it locally

  uv run bench/artbench.py                      # all steps, claude-opus-5-5
  uv run bench/artbench.py --steps create --model claude-sonnet-5-5
  uv run bench/artbench.py --write-baseline     # replace baseline.json with this run

Set `CLAUDE_CODE_OAUTH_TOKEN` (from `claude setup-token`) to run Claude with an empty config
directory, as CI does. Without it, artbench uses your logged-in CLI, which can still load your
user-level files such as `~/.claude/CLAUDE.md`, and marks the report as not comparable to the
baseline.

Write a new baseline from a CI run: download its artifact and copy its `report.json` to
`bench/baseline.json`.
"""

from __future__ import annotations

import argparse
import colorsys
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Callable

from PIL import Image, ImageSequence

ROOT = Path(__file__).resolve().parent.parent
BASELINE = Path(__file__).resolve().parent / "baseline.json"
MARKER = "<!-- artbench -->"
JUDGE_MODEL = "claude-sonnet-5-5"
STEP_TIMEOUT_S = 15 * 60
COST_TOLERANCE = 0.02
COST_WARN_RATIO = 1.5
JUDGE_DROP = 2
ALLOWED_TOOLS = "Bash(ramose *),Read,Write,Edit,Glob,Grep,Skill"


@dataclass(frozen=True)
class Price:
    input: float
    output: float
    cache_read: float

    @property
    def cache_write_5m(self) -> float:
        return self.input * 1.25

    @property
    def cache_write_1h(self) -> float:
        return self.input * 2


PRICES = {
    "claude-opus-5-5": Price(4, 20, 0.20),
    "claude-sonnet-5-5": Price(2, 10, 0.10),
    "claude-haiku-5-5": Price(0.10, 0.50, 0.01),
    "claude-fable-5-1": Price(10, 50, 0.25),
}

BUCKETS = ("input", "cache_write_5m", "cache_write_1h", "cache_read", "output")


@dataclass
class Usage:
    input: int = 0
    cache_write_5m: int = 0
    cache_write_1h: int = 0
    cache_read: int = 0
    output: int = 0

    def __iadd__(self, other: Usage) -> Usage:
        for b in BUCKETS:
            setattr(self, b, getattr(self, b) + getattr(other, b))
        return self


@dataclass
class Cost:
    input: float = 0.0
    cache_write: float = 0.0
    cache_read: float = 0.0
    output: float = 0.0

    @property
    def total(self) -> float:
        return self.input + self.cache_write + self.cache_read + self.output

    def __iadd__(self, other: Cost) -> Cost:
        self.input += other.input
        self.cache_write += other.cache_write
        self.cache_read += other.cache_read
        self.output += other.output
        return self

    def to_json(self) -> dict:
        return {**asdict(self), "total": self.total}


def price_of(model: str) -> Price | None:
    return PRICES.get(model.split("[")[0])


def cost_of(model: str, u: Usage) -> Cost:
    p = price_of(model)
    if p is None:
        return Cost()
    m = 1e6
    return Cost(
        input=u.input * p.input / m,
        cache_write=(u.cache_write_5m * p.cache_write_5m + u.cache_write_1h * p.cache_write_1h) / m,
        cache_read=u.cache_read * p.cache_read / m,
        output=u.output * p.output / m,
    )


@dataclass
class Metered:
    usage: dict[str, Usage]
    cost: Cost
    cli_cost_usd: float
    warnings: list[str]


def meter(result: dict) -> Metered:
    top = result.get("usage") or {}
    split = top.get("cache_creation") or {}
    h1, m5 = split.get("ephemeral_1h_input_tokens", 0), split.get("ephemeral_5m_input_tokens", 0)
    share_1h = h1 / (h1 + m5) if h1 + m5 else 0.0
    usage: dict[str, Usage] = {}
    cost = Cost()
    warnings = []
    for model, mu in (result.get("modelUsage") or {}).items():
        written = mu.get("cacheCreationInputTokens", 0)
        w1h = round(written * share_1h)
        usage[model] = Usage(
            input=mu.get("inputTokens", 0),
            cache_write_5m=written - w1h,
            cache_write_1h=w1h,
            cache_read=mu.get("cacheReadInputTokens", 0),
            output=mu.get("outputTokens", 0),
        )
        if price_of(model) is None:
            warnings.append(f"no price for model {model}")
        cost += cost_of(model, usage[model])
    cli = float(result.get("total_cost_usd") or 0.0)
    if cli and abs(cost.total - cli) > COST_TOLERANCE * cli:
        warnings.append(f"computed cost ${cost.total:.5f} differs from claude total_cost_usd ${cli:.5f} by more than {COST_TOLERANCE:.0%}")
    return Metered(usage, cost, cli, warnings)


CheckFn = Callable[[Path, dict], "tuple[bool, str]"]


@dataclass(frozen=True)
class Check:
    name: str
    fn: CheckFn


@dataclass(frozen=True)
class Step:
    id: str
    prompt: str
    checks: tuple[Check, ...]
    image: str = "assets/mushroom.png"


@dataclass
class Judge:
    score: int = 0
    notes: str = ""
    usage: dict[str, Usage] = field(default_factory=dict)
    cost: Cost = field(default_factory=Cost)


@dataclass
class StepResult:
    id: str
    checks: dict[str, dict]
    judge: Judge
    usage: dict[str, Usage]
    cost: Cost
    cli_cost_usd: float
    num_turns: int
    duration_s: float
    is_error: bool
    warnings: list[str] = field(default_factory=list)


@dataclass
class Report:
    model: str
    claude_version: str
    ramose_commit: str
    auth: str
    steps: list[StepResult]
    totals: dict
    warnings: list[str]
    comparison: dict = field(default_factory=dict)


def sh(project: Path, *args: str, env: dict | None = None) -> subprocess.CompletedProcess:
    return subprocess.run(args, cwd=project, env=env, capture_output=True, text=True)


def ramose_ok(*sub: str) -> CheckFn:
    def run(project: Path, state: dict) -> tuple[bool, str]:
        r = sh(project, "ramose", *sub, env=state["env"])
        text = (r.stdout + r.stderr).strip()
        return r.returncode == 0, text if len(text) <= 1400 else f"{text[:700]}\n...\n{text[-700:]}"

    return run


def png(project: Path, rel: str = "assets/mushroom.png") -> Image.Image | None:
    p = project / rel
    return Image.open(p).convert("RGBA") if p.exists() else None


def coverage(img: Image.Image) -> float:
    a = img.getchannel("A")
    return sum(1 for v in a.get_flattened_data() if v > 128) / (img.width * img.height)


def hue_counts(img: Image.Image) -> tuple[int, int]:
    red = blue = 0
    for r, g, b, a in img.get_flattened_data():
        if a <= 128:
            continue
        h, s, _ = colorsys.rgb_to_hsv(r / 255, g / 255, b / 255)
        deg = h * 360
        if s <= 0.35:
            continue
        if deg < 20 or deg > 340:
            red += 1
        elif 190 <= deg <= 260:
            blue += 1
    return red, blue


def corners(img: Image.Image) -> list[tuple[int, int, int, int]]:
    w, h = img.size
    return [img.getpixel(xy) for xy in ((0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1))]


def need_png(fn: Callable[[Image.Image, dict], tuple[bool, str]]) -> CheckFn:
    def run(project: Path, state: dict) -> tuple[bool, str]:
        img = png(project)
        return fn(img, state) if img else (False, "assets/mushroom.png missing")

    return run


def c_size(img, state):
    return img.size == (512, 512), f"size {img.size}"


def c_coverage(img, state):
    cov = coverage(img)
    state.setdefault("coverage", cov)
    return 0.10 <= cov <= 0.85, f"coverage {cov:.1%}"


def c_corners_clear(img, state):
    px = corners(img)
    return all(p[3] < 16 for p in px), f"corner alpha {[p[3] for p in px]}"


def c_red_over_blue(img, state):
    red, blue = hue_counts(img)
    return red > blue, f"red {red}, blue {blue}"


def c_blue_over_red(img, state):
    red, blue = hue_counts(img)
    return blue > red, f"red {red}, blue {blue}"


def c_coverage_stable(img, state):
    base, cov = state.get("coverage"), coverage(img)
    if base is None:
        return False, "no coverage recorded from the create step"
    return abs(cov - base) <= 0.25 * base, f"coverage {cov:.1%} vs create {base:.1%}"


NAVY = (29, 43, 83)


def c_navy_corners(img, state):
    px = corners(img)
    ok = all(p[3] == 255 and all(abs(p[i] - NAVY[i]) <= 6 for i in range(3)) for p in px)
    return ok, f"corners {px}"


def c_foreground(img, state):
    near = sum(1 for p in img.get_flattened_data() if p[3] < 255 or any(abs(p[i] - NAVY[i]) > 6 for i in range(3)))
    frac = near / (img.width * img.height)
    return frac >= 0.08, f"non-background {frac:.1%}"


def c_exists(*rels: str) -> CheckFn:
    def run(project: Path, state: dict) -> tuple[bool, str]:
        missing = [r for r in rels if not (project / r).exists()]
        return not missing, f"missing {missing}" if missing else "present"

    return run


def c_gif(project: Path, state: dict) -> tuple[bool, str]:
    p = project / "assets/mushroom_idle.gif"
    if not p.exists():
        return False, "assets/mushroom_idle.gif missing"
    with Image.open(p) as gif:
        frames = [f.convert("RGB").tobytes() for f in ImageSequence.Iterator(gif)]
    return len(frames) >= 4, f"{len(frames)} frames"


def c_gif_moves(project: Path, state: dict) -> tuple[bool, str]:
    p = project / "assets/mushroom_idle.gif"
    if not p.exists():
        return False, "assets/mushroom_idle.gif missing"
    with Image.open(p) as gif:
        distinct = len({f.convert("RGB").tobytes() for f in ImageSequence.Iterator(gif)})
    return distinct >= 2, f"{distinct} distinct frames"


BUILD = Check("build", ramose_ok("build"))
CHECK = Check("check", ramose_ok("check"))

STEPS = (
    Step(
        "create",
        "Use ramose to draw a cute chibi mushroom character for a game: a red cap with white spots, a cream stem, "
        "and a simple happy face. Put it in `art/mushroom.ramo` with a 512x512 transparent PNG target `mushroom.png` "
        "and an SVG target `mushroom.svg`. Build it.",
        (
            CHECK,
            BUILD,
            Check("png-512", need_png(c_size)),
            Check("svg-exists", c_exists("assets/mushroom.svg")),
            Check("coverage", need_png(c_coverage)),
            Check("corners-transparent", need_png(c_corners_clear)),
            Check("red-cap", need_png(c_red_over_blue)),
        ),
    ),
    Step(
        "recolor",
        "Make the mushroom's cap blue instead of red, keep the spots and everything else.",
        (BUILD, Check("blue-cap", need_png(c_blue_over_red)), Check("coverage-stable", need_png(c_coverage_stable))),
    ),
    Step(
        "background",
        "Give the mushroom PNG a solid #1d2b53 background instead of transparent.",
        (BUILD, Check("navy-corners", need_png(c_navy_corners)), Check("foreground", need_png(c_foreground))),
    ),
    Step(
        "animate",
        "Add a 1 second looping idle animation where the mushroom gently bobs up and down, written to "
        "`assets/mushroom_idle.gif`. Keep the PNG and SVG.",
        (
            BUILD,
            Check("gif-frames", c_gif),
            Check("gif-moves", c_gif_moves),
            Check("png-svg-kept", c_exists("assets/mushroom.png", "assets/mushroom.svg")),
        ),
    ),
)

FMT = Check("fmt-clean", ramose_ok("fmt", "--check"))

RUBRIC = (
    "You are judging one image from an art-generation benchmark. The user request was:\n\n{prompt}\n\n"
    "Read the image at {path} with the Read tool, then score it from 1 to 10 against this rubric, 2 or 3 points each: "
    "matches the request; reads as a cute chibi mushroom; clean shapes with no artifacts; shading and outline polish. "
    "Be strict: 5 is acceptable, 8 or more is good production art. Reply with the score and short notes."
)
JUDGE_SCHEMA = {
    "type": "object",
    "properties": {"score": {"type": "integer", "minimum": 1, "maximum": 10}, "notes": {"type": "string"}},
    "required": ["score", "notes"],
}


def claude_env(config_dir: Path | None, bin_dir: Path | None) -> dict:
    env = dict(os.environ)
    if config_dir:
        env["CLAUDE_CONFIG_DIR"] = str(config_dir)
    if bin_dir:
        env["PATH"] = f"{bin_dir}{os.pathsep}{env['PATH']}"
    return env


def claude(prompt: str, cwd: Path, env: dict, model: str, tools: str, schema: dict | None = None) -> dict:
    cmd = [
        "claude", "-p", "--model", model, "--output-format", "json",
        "--setting-sources", "project", "--strict-mcp-config", "--allowedTools", tools,
        "--permission-mode", "acceptEdits", "--no-session-persistence",
    ]
    if schema:
        cmd += ["--json-schema", json.dumps(schema)]
    try:
        r = subprocess.run(cmd, input=prompt, cwd=cwd, env=env, capture_output=True, text=True, timeout=STEP_TIMEOUT_S)
        return json.loads(r.stdout)
    except subprocess.TimeoutExpired:
        return {"is_error": True, "result": f"timeout after {STEP_TIMEOUT_S}s"}
    except json.JSONDecodeError:
        return {"is_error": True, "result": f"unparseable claude output: {r.stdout[-300:]!r} {r.stderr[-300:]!r}"}


def judge(step: Step, project: Path, env: dict, scratch: Path) -> Judge:
    src = png(project, step.image)
    if src is None:
        return Judge(notes=f"skipped, {step.image} missing")
    flat = Image.new("RGBA", src.size, (204, 204, 204, 255))
    flat.alpha_composite(src)
    shot = scratch / f"{step.id}.png"
    flat.convert("RGB").save(shot)
    prompt = RUBRIC.format(prompt=step.prompt, path=shot)
    result = claude(prompt, scratch, env, JUDGE_MODEL, "Read", JUDGE_SCHEMA)
    m = meter(result)
    out = result.get("structured_output") or {}
    if "score" not in out:
        return Judge(notes=f"judge failed: {str(result.get('result'))[:200]}", usage=m.usage, cost=m.cost)
    return Judge(int(out["score"]), str(out.get("notes", "")), m.usage, m.cost)


def run_step(step: Step, project: Path, env: dict, scratch: Path, state: dict, model: str) -> StepResult:
    t0 = time.monotonic()
    result = claude(step.prompt, project, env, model, ALLOWED_TOOLS)
    elapsed = time.monotonic() - t0
    m = meter(result)
    state["env"] = env
    checks = {}
    for c in (*step.checks, FMT):
        ok, detail = c.fn(project, state)
        checks[c.name] = {"ok": ok, "detail": detail}
    failed = bool(result.get("is_error"))
    if failed:
        checks["claude-run"] = {"ok": False, "detail": str(result.get("result"))[:400]}
    return StepResult(
        step.id, checks, judge(step, project, env, scratch), m.usage, m.cost, m.cli_cost_usd,
        int(result.get("num_turns") or 0), round(elapsed, 1), failed, m.warnings,
    )


def sum_usage(rs: list[StepResult]) -> Usage:
    total = Usage()
    for r in rs:
        for u in r.usage.values():
            total += u
    return total


def totals(rs: list[StepResult]) -> dict:
    cost = Cost()
    for r in rs:
        cost += r.cost
    judge_cost = Cost()
    for r in rs:
        judge_cost += r.judge.cost
    return {
        "usage": asdict(sum_usage(rs)),
        "cost": cost.to_json(),
        "judge_cost_usd": judge_cost.total,
        "num_turns": sum(r.num_turns for r in rs),
        "duration_s": round(sum(r.duration_s for r in rs), 1),
    }


def to_json(report: Report) -> dict:
    return asdict(report) | {
        "steps": [
            asdict(s) | {"cost": s.cost.to_json(), "judge": asdict(s.judge) | {"cost": s.judge.cost.to_json()}}
            for s in report.steps
        ]
    }


def slim(doc: dict) -> dict:
    keep = ("model", "ramose_commit", "totals")
    out = {k: doc[k] for k in keep}
    out["steps"] = [
        {
            "id": s["id"],
            "checks": {n: {"ok": c["ok"]} for n, c in s["checks"].items()},
            "judge": {"score": s["judge"]["score"]},
            "cost": s["cost"],
            "num_turns": s["num_turns"],
        }
        for s in doc["steps"]
    ]
    return out


def compare(doc: dict, base: dict | None) -> dict:
    if base is None:
        return {"baseline": False, "regressed": False, "steps": {}}
    if base["model"] != doc["model"]:
        return {"baseline": False, "regressed": False, "steps": {}, "skipped": f"baseline is {base['model']}"}
    by_id = {s["id"]: s for s in base["steps"]}
    steps = {}
    for s in doc["steps"]:
        b = by_id.get(s["id"])
        if b is None:
            continue
        regressions = [f"check `{n}` passed in baseline and now fails" for n, c in b["checks"].items()
                       if c["ok"] and not s["checks"].get(n, {}).get("ok")]
        if s["judge"]["score"] < b["judge"]["score"] - JUDGE_DROP:
            regressions.append(f"judge score {s['judge']['score']} vs baseline {b['judge']['score']}")
        ratio = s["cost"]["total"] / b["cost"]["total"] if b["cost"]["total"] else None
        warnings = [f"cost {ratio:.2f}x baseline"] if ratio and ratio > COST_WARN_RATIO else []
        steps[s["id"]] = {
            "regressions": regressions, "warnings": warnings,
            "cost_delta_pct": (ratio - 1) * 100 if ratio else None,
            "judge_delta": s["judge"]["score"] - b["judge"]["score"],
            "base_cost": b["cost"],
        }
    return {"baseline": True, "regressed": any(v["regressions"] for v in steps.values()), "steps": steps}


def pct(v: float | None) -> str:
    return "" if v is None else f" ({v:+.0f}%)"


def tokens(u: dict) -> tuple[int, int, int, int]:
    return u["input"], u["cache_write_5m"] + u["cache_write_1h"], u["cache_read"], u["output"]


def cost_cell(c: dict, delta: float | None) -> str:
    return f"{c['input']:.3f} / {c['cache_write']:.3f} / {c['cache_read']:.3f} / {c['output']:.3f} = **${c['total']:.3f}**{pct(delta)}"


def summary(doc: dict) -> str:
    cmp = doc["comparison"]
    failed = [s for s in doc["steps"] if not all(c["ok"] for c in s["checks"].values())]
    lines = [MARKER, "## artbench", ""]
    verdict = "REGRESSION" if cmp["regressed"] else "FAIL" if failed else "PASS"
    note = "" if cmp["baseline"] else f" ({cmp.get('skipped', 'no baseline')}, nothing compared)"
    lines += [f"**{verdict}**{note}. model `{doc['model']}`, claude {doc['claude_version']}, ramose `{doc['ramose_commit'][:9]}`, auth `{doc['auth']}`.", ""]
    lines += [
        "| step | checks | judge | input | cache write | cache read | output | cost USD in / write / read / out | turns | time |",
        "|---|---|---|---|---|---|---|---|---|---|",
    ]
    for s in doc["steps"]:
        c = cmp["steps"].get(s["id"], {})
        ok = sum(x["ok"] for x in s["checks"].values())
        jd = f" ({c['judge_delta']:+d})" if "judge_delta" in c else ""
        i, w, r, o = tokens(sum_dicts(s["usage"]))
        lines.append(f"| {s['id']} | {ok}/{len(s['checks'])} | {s['judge']['score']}{jd} | {i:,} | {w:,} | {r:,} | {o:,} | "
                     f"{cost_cell(s['cost'], c.get('cost_delta_pct'))} | {s['num_turns']} | {s['duration_s']:.0f}s |")
    t = doc["totals"]
    i, w, r, o = tokens(t["usage"])
    lines.append(f"| **total** | | | {i:,} | {w:,} | {r:,} | {o:,} | {cost_cell(t['cost'], total_delta(doc))} | {t['num_turns']} | {t['duration_s']:.0f}s |")
    lines += ["", f"Judge calls (not in totals): ${t['judge_cost_usd']:.3f}.", ""]
    notes = [f"- {w}" for w in doc["warnings"]]
    for sid, c in cmp["steps"].items():
        notes += [f"- {sid}: regression, {x}" for x in c["regressions"]]
        notes += [f"- {sid}: warning, {x}" for x in c["warnings"]]
    if notes:
        lines += ["**Notes**", "", *notes, ""]
    lines += ["<details><summary>Failed checks and judge notes</summary>", ""]
    for s in doc["steps"]:
        lines.append(f"**{s['id']}**")
        lines += [f"- FAIL `{n}`: {c['detail']}" for n, c in s["checks"].items() if not c["ok"]]
        lines += [f"- judge: {s['judge']['notes']}", ""]
    lines += ["</details>", ""]
    return "\n".join(lines)


def sum_dicts(usage: dict[str, dict]) -> dict:
    return {b: sum(u[b] for u in usage.values()) for b in BUCKETS}


def total_delta(doc: dict) -> float | None:
    steps = doc["comparison"]["steps"]
    if not steps:
        return None
    base = sum(v["base_cost"]["total"] for v in steps.values())
    now = sum(s["cost"]["total"] for s in doc["steps"] if s["id"] in steps)
    return (now / base - 1) * 100 if base else None


def git_commit() -> str:
    r = subprocess.run(["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True)
    return r.stdout.strip() or "unknown"


def resolve_ramose(path: str | None) -> Path:
    if path:
        return Path(path).resolve()
    subprocess.run(["cargo", "build", "--release", "-p", "ramose"], cwd=ROOT, check=True)
    return ROOT / "target/release/ramose"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--model", default="claude-opus-5-5")
    ap.add_argument("--ramose")
    ap.add_argument("--steps")
    ap.add_argument("--out", default=str(ROOT / "bench/out"))
    ap.add_argument("--write-baseline", action="store_true")
    args = ap.parse_args()

    wanted = args.steps.split(",") if args.steps else [s.id for s in STEPS]
    unknown = set(wanted) - {s.id for s in STEPS}
    if unknown:
        ap.error(f"unknown steps {sorted(unknown)}")
    steps = [s for s in STEPS if s.id in wanted]
    binary = resolve_ramose(args.ramose)
    out = Path(args.out).resolve()
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)

    isolated = bool(os.environ.get("CLAUDE_CODE_OAUTH_TOKEN"))
    results: list[StepResult] = []
    with tempfile.TemporaryDirectory(prefix="artbench-") as tmp:
        tmp = Path(tmp)
        project, scratch, config = tmp / "project", tmp / "judge", tmp / "config"
        project.mkdir(), scratch.mkdir(), config.mkdir()
        env = claude_env(config if isolated else None, binary.parent)
        subprocess.run([str(binary), "init"], cwd=project, check=True, env=env, capture_output=True)
        (project / "art/hello.ramo").unlink()
        shutil.copytree(ROOT / "plugins/ramose/skills/ramose", project / ".claude/skills/ramose")
        state: dict = {}
        for step in steps:
            print(f"step {step.id} ...", file=sys.stderr, flush=True)
            r = run_step(step, project, env, scratch, state, args.model)
            results.append(r)
            print(f"  {r.duration_s:.0f}s ${r.cost.total:.3f} judge {r.judge.score}", file=sys.stderr, flush=True)
            for sub in ("art", "assets"):
                if (project / sub).exists():
                    shutil.copytree(project / sub, out / "steps" / step.id / sub)
    version = subprocess.run(["claude", "--version"], capture_output=True, text=True).stdout.strip()
    warnings = [] if isolated else ["auth: local-login, numbers are not comparable to the baseline"]
    warnings += [f"{r.id}: {w}" for r in results for w in r.warnings]
    report = Report(args.model, version, git_commit(), "oauth-token" if isolated else "local-login", results, totals(results), warnings)
    doc = to_json(report)
    base = json.loads(BASELINE.read_text()) if BASELINE.exists() else None
    doc["comparison"] = compare(doc, base)
    (out / "report.json").write_text(json.dumps(doc, indent=2) + "\n")
    text = summary(doc)
    (out / "summary.md").write_text(text)
    print(text)
    if args.write_baseline:
        BASELINE.write_text(json.dumps(slim(doc), indent=2) + "\n")
    all_ok = all(c["ok"] for s in doc["steps"] for c in s["checks"].values())
    return 0 if all_ok and not doc["comparison"]["regressed"] else 1


if __name__ == "__main__":
    sys.exit(main())
