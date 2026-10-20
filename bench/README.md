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

`wow.py` measures how good a first `/ramose <prompt>` result looks. It runs N clean `claude -p` sessions that load only `plugins/ramose` and the ramose binary built from this checkout, so a change to the skill, the docs, or the std library is what it measures. Each PNG goes to two judges, opus and sonnet, which score it from 1 to 20 on fidelity, composition, color, form, polish, and wow. A trial's composite is the mean of both judges over the six axes. A third judge, fable, was dropped: it was the noisiest on a re-judge of the same images, it agreed least with the other two, and as the middle scorer it set the median in 80 of 104 trials.

```sh
uv run bench/wow.py --label base                  # 6 trials of the fox prompt
uv run bench/wow.py --label base --prompt chest   # the treasure chest icon prompt
uv run bench/wow.py --label ctl --judge-only a.png b.png
```

Results, PNGs, and the `.ramo` sources go to `bench/out/wow/<label>-<prompt>/`. One trial's composite moves by about 0.5 from run to run, so compare means of at least 6 trials, and run both prompts before you trust a change.
