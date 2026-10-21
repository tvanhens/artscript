# Compare Codex artwork quality

`paired.py` generates new artwork and compares it with six saved reference images. All author and judge calls use Codex. Judges score both images again, anonymously and in both orientations. Historical scores are context, never the baseline for a new score subtraction.

## Compare with saved generations

```sh
cargo build --release -p ramose
uv run bench/paired.py run \
  --reference bench/references/quality-v1 \
  --candidate plugins/ramose/skills/ramose \
  --ramose target/release/ramose \
  --model gpt-6.1-sol --effort medium \
  --judge-model gpt-6.1-sol --judge-effort medium \
  --repetitions 5 --jobs 2 \
  --out bench/out/paired/round-01
```

The reference contains exact PNGs, prompts, original scores and source identifiers. It contains no skill snapshots or generated projects. Prompts come from its manifest. Each repetition creates one candidate per prompt and compares it with the same saved image. Reference images have no author calls, rebuilds or measured delivery rate. Reports count reference uses separately from new successful deliveries. This is a fixed-image regression test, not an estimate of how reliably the old authoring setup would work today.

The checked-in reference contains the latest complete six-prompt batch using the shipped contour guidance. Both author and judge used `gpt-6.1-sol` with `medium` reasoning effort. Selection used recency, independent of scores. Its source skill commit identifies the unchanged accepted guidance. The reference fingerprint identifies the exact images and provenance. These six examples support regression checks; they do not establish general authoring reliability.

Model availability depends on the authenticated Codex account. Strict runs require explicit author and judge models and reasoning efforts. Requested identifiers do not prove an immutable server-side model. `--exploratory` permits defaults for a smoke test but cannot enter longitudinal history.

## Replace the reference explicitly

After reviewing a complete sealed run, export a specified repetition to a new directory. Selection is explicit and never picks the highest-scoring image automatically. An unsuccessful selected delivery blocks export.

```sh
uv run bench/paired.py reference bench/out/paired/round-01 \
  --arm candidate --repetition 4 \
  --source-commit FULL_GIT_COMMIT_OF_THE_CANDIDATE_SKILL \
  --out bench/references/quality-v2
uv run bench/paired.py verify-reference bench/references/quality-v2
```

Repetition indices start at zero. The source commit identifies the skill version, not the time of image capture. Keep the recorded skill and binary hashes when evaluating uncommitted changes. Export refuses an existing destination. Review and commit the new manifest and images, then use that directory in future runs. Old images can leave the current tree once no longer used. Git history retains them. Compact result records keep their reference fingerprint, and different references form separate history groups.

## Measure fresh authoring on both sides

Use fresh pairs to measure baseline delivery reliability, generation variability, or a broader suite. Keep an unchanged baseline skill and binary for the series.

```sh
uv run bench/paired.py run \
  --baseline /path/to/reference/ramose \
  --candidate plugins/ramose/skills/ramose \
  --ramose target/release/ramose \
  --prompts bench/suites/quality-v2.json \
  --model gpt-6.1-sol --effort medium \
  --judge-model gpt-6.1-sol --judge-effort medium \
  --repetitions 5 --jobs 2 \
  --out bench/out/paired/fresh-01
```

`--baseline` and `--reference` are mutually exclusive. `--ramose` supplies both fresh-arm binaries by default. `--candidate-ramose` can select a different candidate binary. In reference mode only the candidate binary is copied. The author chooses renderer options. Clean validation uses that binary and host with default backend selection. A requested nondefault backend can cause a rebuild mismatch.

The broader suite contains 24 tagged still-image requests. The saved reference covers the original six in `quality-prompts.json`. Published holdouts are regression cases, not unseen confirmation prompts. Reserve fresh prompts to confirm a newly selected winner. Custom fresh-pair suites may map IDs to nonempty prompts or use the tagged versioned format.

Explicit `[$ramose](absolute-skill-path) <prompt>` invocation selects the intended skill. This tests still artwork delivery. Bare-name discovery and animation are outside the protocol. Codex ignores user configuration and runs in a fresh temporary directory with full filesystem access. Use trusted skills. Each project must deliver one canonical PNG. Numbered scale companions such as `@2x` are allowed. Several unrelated outputs fail delivery.

## Interpret the result

Each prompt has the same number of repetitions. For fresh pairs, baseline and candidate launch order alternates by repetition. Judges see anonymous PNG attachments in both orientations. Scores average six axes on a 1–20 scale and then average the two orientations.

The primary estimate weights each prompt equally. Its interval resamples prompt means with a deterministic percentile bootstrap. It describes variability over this curated prompt set, not a guaranteed population interval. A one-prompt run has no prompt-cluster interval. `fixed_suite_pair_ci95` separately resamples individual pairs and only describes repeated trials on that fixed suite. The report also retains per-prompt results, preference votes and orientation disagreements.

Completed authors with invalid artwork receive zero and remain in the denominator. Provider failures, missing completed turns, unavailable processes, timeouts and unknown harness failures make the comparison inconclusive. They do not become low art scores. Missing or invalid judges also block conclusions. `successful_pair_delta_mean` is a conditional diagnostic over pairs with two valid deliveries. It is not a substitute for the delivery-inclusive primary estimate.

Author durations and available token fields are recorded, along with the number of observations. Reused authoring is separate. The report also sums known author and judge receipt usage, including archived attempts and judge retries, with missing observations counted. Interrupted calls that never wrote a receipt remain unobserved. There is no fabricated monetary cost estimate. Keep models, effort, concurrency and generation budgets fixed when comparing alternatives. The protocol generates one new image per authored arm and repetition. A multi-candidate workflow needs a new budgeted protocol.

Choose an acceptance threshold before evaluating candidates. Repeated selection over the same suite can overfit it. Separate Codex sessions do not provide model-family independence. Automated scores need human calibration before treating them as a reliable proxy for expert preference.

## Verify, resume and replay

Ignored run directories retain candidate inputs, optional fresh baseline inputs, evaluator sources, scoring definition and effective judge instructions, failure and retry policy, environment versions, prompts, schedules, transcripts, projects, images and validation receipts. Artifact references are relative. Copy the whole directory to relocate it. Generated source is rebuilt from an empty output directory and its pixels are checked against the delivered image.

```sh
uv run bench/paired.py verify bench/out/paired/round-01
uv run bench/paired.py report bench/out/paired/round-01
uv run bench/paired.py resume bench/out/paired/round-01
```

A sealed inventory covers every retained file except the root report, seal and lock. Missing, unexpected, modified, escaping and symlinked files are rejected. Report recomputes aggregates and checks scores against retained judge outputs. Invalid protocol evidence suppresses all numeric conclusions, including per-prompt results. The seal detects changes relative to a published digest. It is not a signature or proof against someone replacing both data and digest.

Stop the previous runner and its model processes before resuming. An interrupt in the coordinating process can wait for active worker timeouts. Resume checks frozen inputs and completed receipts, then fills absent or interrupted work. Terminal author outcomes and completed judgments are preserved. Interrupted attempts are archived. A completed sealed run is a no-op. Concurrent writers are rejected. Unsealed runs require the captured evaluator implementation for resume. Run it explicitly with `PYTHONDONTWRITEBYTECODE=1` if using the retained `evaluator/paired.py`, so Python does not add untracked cache files inside the bundle.

To rejudge captured work after a judge outage or evaluator correction, repeat `run` with matching author inputs and `--replay bench/out/paired/round-01`, plus a new output directory. The source must have a valid seal and a complete schedule of captured terminal authors or reference receipts. A fixed reference must match the original fingerprint. Replay runs no new authors, preserves their non-success states, validates again and judges every eligible pair. Original bundles remain immutable. Replayed scores belong to the new evaluator protocol.

New report implementations identify their analysis version separately from the frozen evaluator. Exact historical analysis can be rerun with the retained evaluator sources. Legacy unversioned runs require their original frozen evaluator and cannot be relabeled as strict v2 experiments.

## Track changes

```sh
uv run bench/bundle.py record bench/out/paired/round-01 \
  --out bench/history/round-01.json
uv run bench/bundle.py history bench/history/*.json
```

History records are immutable compact JSON files. Recording requires a verified, complete strict run. Keep records outside the run bundle. Groups match the protocol, suite, compute settings and environment. Fixed-image runs must share the reference fingerprint. Fresh pairs must share the baseline skill and binary. Candidate hashes may differ. Compare paired candidate-minus-reference deltas within a group. Absolute judge scores from different paired contexts or unmatched baselines are not a trend.

To combine disjoint prompt batches, run `uv run bench/combine_paired.py first/run second/run --out bench/out/paired/combined/report.json`. The command verifies each source bundle and recomputes its report. All protocol fields except the suite hash must match, as must both treatments. Duplicate prompt IDs are rejected. A combined report is an analysis artifact, not a new sealed run or history entry.

## Historical hillclimb

[The compact result record](results/hillclimb-2026-10.json) preserves all eleven comparisons, counts and original and retrospective intervals. The full experiment archive remains in the recorded Git commit, rather than in the maintained benchmark tree.

| Comparison | Candidate minus baseline | Prompt-cluster 95% interval |
| --- | ---: | --- |
| Contour guidance, six development prompts | +0.52 | [0.25, 0.79] |
| Contour guidance, three held-out prompts | +0.77 | [-0.20, 2.22] |
| Medium guidance, three held-out prompts | +0.16 | [-0.30, 0.45] |

The contour holdout gain concentrated in the kingfisher prompt. Its prompt-level interval includes zero. These exploratory results used unresolved model defaults and uncalibrated Codex judges. They do not establish a general quality improvement or a pinned trend baseline. One provider capacity failure received zero under the old protocol. Current runs treat provider failures as inconclusive.

## Calibrate with human preferences

```sh
uv run bench/human_review.py export bench/out/paired/round-01 bench/out/human/round-01
uv run bench/human_review.py import bench/out/human/round-01 /path/to/completed-answers.json
```

Share only the packet's `public/` directory with reviewers. Its HTML shows anonymous images and prompts. Reviewers fill the provided answer schema with a reviewer identifier and left, right or tie preferences. Keep `private.json` private until annotation is finished. Imports validate item identities and retained images, and write immutable `calibrations/` receipts tied to the exact packet and answers. No annotations means calibration is absent. Agreement is reported against unanimous machine orientation votes. Observed left/right counts describe position preference, not measured swap sensitivity. Multiple reviewers and repeated blinded judgments are needed to assess human reliability.

## Test the benchmark

```sh
PYTHONDONTWRITEBYTECODE=1 uv run --with pillow==11.3.0 python -m unittest discover -s bench -p 'test_*.py'
```

The no-model CI job exercises receipts, invalid inputs, clustering, sealing, relocation, resume, fake executable Codex and renderer processes, history and human annotation import. It uses no model credentials. Live evaluation remains an explicit local command.

# Older benchmark entry points

The following tools predate the Codex protocol and retain their existing Claude workflows. They are not used by the comparisons above.

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
