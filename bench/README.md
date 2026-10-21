# artbench

artbench checks that the ramose skill in `plugins/ramose/skills/ramose` still makes good art, and measures what it costs. It runs Claude Code (`claude -p`) through one fixed scenario in a scratch project: draw a chibi mushroom, then recolor the cap, add a background, and add an idle GIF. Each step is a new session.

For each step, artbench records:

- Deterministic checks: `check`, `build`, and `fmt --check` pass, the outputs exist at the right size, and pixel tests confirm the requested change.
- A judge score from 1 to 10, given by a separate `claude -p` call that looks at the PNG.
- Tokens and cost, split into input, cache write, cache read, and output.

A run fails when a check fails, when a check that passed in `baseline.json` now fails, or when a judge score drops more than 2 below the baseline. A step that costs more than 1.5 times its baseline gets a warning.

## Run it on a pull request

Comment `@artbench` on the pull request. To use a different model, add it, for example `@artbench model=claude-sonnet-5-5`. Only owners, members, and collaborators can start a run. The workflow posts the summary as a comment and uploads the art and `report.json` as an artifact.

GitHub runs a workflow for a plain PR comment only from the default branch. Until `artbench.yml` is on `main`, start a run with a review comment instead. The review events use the pull request's copy of the workflow.

## Run it locally

```sh
uv run bench/artbench.py                      # all steps, claude-opus-5-5
uv run bench/artbench.py --steps create --model claude-sonnet-5-5
uv run bench/artbench.py --write-baseline     # replace baseline.json with this run
```

Set `CLAUDE_CODE_OAUTH_TOKEN` (from `claude setup-token`) to run Claude with an empty config directory, as CI does. Without it, artbench uses your logged-in CLI, which can still load your user-level files such as `~/.claude/CLAUDE.md`, and marks the report as not comparable to the baseline.

Write a new baseline from a CI run: download its artifact and copy its `report.json` to `bench/baseline.json`.

# wow

`wow.py` measures how good a first `/ramose <prompt>` result looks. It runs N clean `claude -p` sessions that load only `plugins/ramose` and the ramose binary built from this checkout, so a change to the skill, the docs, or the std library is what it measures. The judges score each image within its style, the one the prompt names or the one the image chose, so no style wins for being that style. Each PNG goes to two judges, opus and sonnet, which score it from 1 to 20 on fidelity, composition, color, form, polish, and wow. A trial's composite is the mean of both judges over the six axes. A third judge, fable, was dropped: it was the noisiest on a re-judge of the same images, it agreed least with the other two, and as the middle scorer it set the median in 80 of 104 trials.

```sh
uv run bench/wow.py --label base                  # 6 trials of the fox prompt
uv run bench/wow.py --label base --prompt chest   # the treasure chest icon prompt
uv run bench/wow.py --label base --prompt lighthouse  # a prompt that names a style
uv run bench/wow.py --label ctl --judge-only a.png b.png
```

Results, PNGs, and the `.ramo` sources go to `bench/out/wow/<label>-<prompt>/`. One trial's composite moves by about 0.5 from run to run, so compare means of at least 6 trials, and run all three prompts before you trust a change.

# Compare Codex skill changes

`paired.py` runs ramose requests in fresh Codex projects with a local skill copy. It uses `[$ramose](absolute-skill-path) <prompt>` to select that copy explicitly. It compares two skill directories against the same ramose binary. It requires Codex and Claude CLI authentication.

Create a JSON file that maps prompt names to requests. Use still-image requests that produce one artwork in the manifest's output directory. A base PNG can have `@2x` or other numbered scale variants.

```json
{
  "fox": "draw a cute fox sitting next to a glowing mushroom",
  "chest": "make a game icon of a treasure chest overflowing with gold coins",
  "lighthouse": "paint a lighthouse on a cliff at dusk in a loose watercolor style"
}
```

Build ramose, then compare a saved baseline skill directory with the changed skill.

```sh
cargo build --release -p ramose
uv run bench/paired.py run \
  --baseline /path/to/baseline/ramose \
  --candidate plugins/ramose/skills/ramose \
  --ramose target/release/ramose \
  --prompts /path/to/prompts.json \
  --repetitions 5 --jobs 2 \
  --out bench/out/paired/round-01
uv run bench/paired.py report bench/out/paired/round-01
```

The runner uses the Codex default model unless you pass `--model`. It ignores user configuration, retains authentication, and runs authoring commands with full filesystem access. Temporary projects separate outputs; they do not sandbox the author. Use only trusted skill directories. The report labels an unspecified model as unresolved.

Each prompt and repetition has two generations. Their launch order alternates. Claude judges the anonymous PNGs together on the six `wow.py` axes, then judges them again with positions reversed. Each generated pair contributes one averaged difference to the bootstrap interval. The report includes prompt-level results, delivery failures, and position disagreements.

Failed deliveries score zero and remain in the denominator. Missing judge results make the comparison incomplete. The runner preserves the delivered project, checks and rebuilds a separate copy with an empty output directory, compares the rebuilt pixels, and checks formatting. It judges the base PNG when scale variants exist. It rejects multiple unrelated PNGs instead of guessing which artwork to judge.

The output directory retains skill and binary snapshots, hashes, schedules, transcripts, sources, images, and validation receipts. A nonempty output directory is never overwritten. Reporting checks that the runner and rubric still match the frozen hashes. Codex JSON events do not expose every skill load or image-viewer call. The runner records observed skill reads, but does not treat absent events as failed delivery.

Choose a meaningful gain and acceptance rule before running candidates. Confirm promising changes on fresh prompts and generations. A positive mean from a few familiar prompts does not establish a general quality improvement.

To remeasure saved generations after correcting an evaluator, repeat the `run` command with `--replay bench/out/paired/round-01` and a new `--out` directory. Replay requires matching prompts, repetitions, skill snapshots, binary, and author model configuration. Every original generation must be captured. It launches no new authors, but runs clean validation and blind judging again for every pair. The report records the source experiment and marks authoring as reused. Discard the old measurements when the evaluator changes.

Run the accounting tests with `uv run --with pillow python -m unittest discover -s bench -p test_paired.py`.
