# Historical ramose skill hillclimb

The contour paragraph passed the original acceptance rule. Its fresh holdout gain was 0.772 points out of 20, or 5.0%. This is exploratory evidence. The retrospective prompt-cluster interval includes zero. These runs are not a pinned baseline for future trends.

This archive preserves 11 comparisons, 300 pairs, and 600 scheduled generations. Every comparison used five repetitions per prompt. There were 599 successful deliveries and one provider-capacity failure. All judgments completed.

## Reproduce the numerical analysis

Run from the repository root with Python 3.11 or newer. No model, network, or third-party package is needed.

```sh
python3 bench/results/hillclimb-2026-10/analyze.py --check
```

The command verifies the archive inventory, the complete pair schedule, every retained judge score against its pair score, all archived source hashes, and every historical aggregate and per-prompt summary. It also recomputes `analysis.json`. Running without `--check` writes that derived file.

The original numerical function is extracted from `frozen-evaluator/paired.py`. The rest of that runner is never executed. The frozen files preserve the original implementation, including unused legacy provider code in `wow.py`. All judgments reported here used Codex.

## Results

Scores average six axes on a 1–20 scale. Failed deliveries score zero. Each successfully delivered pair was judged in both attachment orders. Means average the two orientations. Historical intervals resample pairs. Retrospective intervals resample entire prompt means, preserving the prompt as the unit of generalization. Both use 4,000 bootstrap draws, seed 0, and order-statistic indices 100 and 3899.

| Comparison | Prompts | Baseline | Candidate | Delta | Original pair 95% interval | Retrospective prompt 95% interval |
|---|---:|---:|---:|---:|---|---|
| review-six | 6 | 15.156 | 15.381 | +0.225 | [-0.192, +0.639] | [-0.183, +0.600] |
| plan-six | 6 | 15.242 | 15.406 | +0.164 | [-0.281, +0.606] | [-0.214, +0.536] |
| simplify-six | 6 | 15.897 | 14.806 | -1.092 | [-1.569, -0.617] | [-1.564, -0.628] |
| relationships-six | 6 | 15.472 | 15.053 | -0.419 | [-0.836, -0.003] | [-0.981, +0.072] |
| contours-six | 6 | 15.014 | 15.533 | +0.519 | [+0.131, +0.908] | [+0.250, +0.792] |
| medium-six | 6 | 14.969 | 15.472 | +0.503 | [+0.022, +0.950] | [+0.142, +0.828] |
| contours-medium-six | 6 | 15.314 | 15.303 | -0.011 | [-0.481, +0.436] | [-0.425, +0.328] |
| contours-review-six | 6 | 15.503 | 14.567 | -0.936 | [-2.278, -0.044] | [-2.208, -0.172] |
| contours-plan-six | 6 | 15.486 | 15.108 | -0.378 | [-0.772, +0.042] | [-0.750, +0.139] |
| contours-holdout | 3 | 15.328 | 16.100 | +0.772 | [+0.017, +1.511] | [-0.200, +2.217] |
| medium-holdout | 3 | 16.144 | 16.300 | +0.156 | [-0.439, +0.722] | [-0.300, +0.450] |

The original development gate required at least +0.5 points, a positive lower bound for the paired interval, and no delivery regression. Contours and medium guidance passed development. Contours also passed its fresh holdout. Medium guidance did not pass its separate holdout. The three additions to contours did not pass. That sequence ended the local hillclimb. It does not prove that no further improvement exists.

The contour holdout contained three prompts. Teapot changed by −0.200, robot by +0.300, and kingfisher by +2.217 points. The gain was concentrated in kingfisher. Three prompt clusters cannot establish reliable coverage of the request distribution. The retrospective interval is descriptive and was not the original acceptance test. Repeated adaptive development comparisons also introduce selection bias. No multiple-comparison correction was applied.

The sole failure occurred for the candidate in `contours-review-six`, sailboat repetition 2. Codex reported that its selected model was at capacity after producing artwork. The turn never completed, so the primary score remained zero. Among the 29 pairs where both arms delivered, the delta was −0.394. This sensitivity analysis conditions on delivery and does not replace the primary result.

## Evidence and scope

`evidence.json` contains exact prompts, per-pair judgments, raw judge-output values, original summaries, and experiment provenance. It preserves hashes of the original manifests, pair receipts, source files, and original and rebuilt PNGs. Paths are made portable. A hash for an omitted original file records its identity but cannot verify that file without the local capture.

`skills/` contains the exact skill snapshots keyed by SHA-256. Each experiment manifest maps its baseline and candidate to these snapshots. `frozen-evaluator/` retains the measurement implementation and original file hashes. The historical rubric hash covered the rubric text alone. These records must not be relabeled as the later complete evaluator fingerprint.

`art/` contains all authored `.ramo` files and manifests for both arms of the accepted contour development and holdout comparisons. `sheets/` contains every image from those comparisons, in original repetition order. Baseline is left and candidate is right. The 320px thumbnails are lossy previews on a neutral background, not the judge inputs. Full-size originals and rebuilt PNGs total about 52.5 MB and are omitted. Their hashes remain in the evidence. The complete local captures also retain transcripts, binaries, and validation workspaces. Those large captures are not included here.

The archive inventory detects content changes in the evidence and included artifacts. It is an integrity aid, not an external signature. The original sources can be rendered again with the corresponding renderer, but the binary itself is omitted. Exact numerical replay works offline. Exact historical live generation cannot be recreated.

## Limits for future comparisons

Both author and judge used unresolved Codex defaults through `codex-cli 0.163.0`. Reasoning effort was not pinned. The resolved rendering backend was not recorded as an independent provenance field. Neither the model identity nor its effort can be recovered reliably from these records. Separate sessions do not establish model-family independence. There is no blind human calibration, so score changes are changes in Codex judgments, not verified human preference gains.

The suite measures still-image authoring through explicit local skill selection. It does not measure bare-name skill discovery or animation. Generation was stochastic. Some early authoring captures were replayed through the frozen validation and Codex judging pipeline. Each manifest records that reuse. Model calls, latency, and cost were not controlled well enough for an equal-compute or performance claim.

The original holdouts are now public regression prompts. They must not be treated as unseen confirmation data again. The `longitudinal_eligible` flag is false. Future pinned runs should use the hardened benchmark protocol and a stable reference. Do not join this archive to that trend series merely because both report a 20-point mean.

## Visual evidence

- [contours-holdout-bird](sheets/contours-holdout-bird.jpg)
- [contours-holdout-robot](sheets/contours-holdout-robot.jpg)
- [contours-holdout-tea](sheets/contours-holdout-tea.jpg)
- [contours-six-chest](sheets/contours-six-chest.jpg)
- [contours-six-fern](sheets/contours-six-fern.jpg)
- [contours-six-fox](sheets/contours-six-fox.jpg)
- [contours-six-lighthouse](sheets/contours-six-lighthouse.jpg)
- [contours-six-potion](sheets/contours-six-potion.jpg)
- [contours-six-sailboat](sheets/contours-six-sailboat.jpg)

## Rebuild this archive from retained captures

`export.py` requires the original local `bench/out/paired/` directories and `.audit/hillclimb/frozen-evaluator/`. It exports only the 11 named valid comparisons. It excludes earlier invalid trials and the personal decision log.

```sh
uv run --with pillow python bench/results/hillclimb-2026-10/export.py
python3 bench/results/hillclimb-2026-10/analyze.py
python3 bench/results/hillclimb-2026-10/analyze.py --check
```

The exporter rewrites the evidence inventory. Run it only when intentionally recreating this historical archive from the original captures. Future benchmark results belong in separate result directories.
