# Choosing the core card's size

The language card is in every prompt, so each token in it is paid on every request. A smaller core
with per-topic help is cheaper only if agents don't make up the difference in help lookups, fix-up
turns and worse art. This protocol measures that. It has not been run yet.

## Setups

| setup | prompt holds | help |
|---|---|---|
| `full` | `docs/language.md` (3468 tokens) | none |
| `core1500` | a core of about 1500 tokens | `artscript help <topic>` |
| `core800` | `bench/card_core_800.md` (796 tokens) | `artscript help <topic>` |

`core1500` is `docs/language.md` with these parts moved behind help, then trimmed to 1500 ± 50
tokens: the Fields section, slots and overrides from Components, the check function list, the CLI
section, `@target` keys other than the size, masks, `fx as:/of:/merge`, and the shape table rows
for poly, star, blob, path, mirror, repeat and grid. Write it as `bench/card_core_1500.md`, check its
size with the same o200k_base count, and confirm its code evaluates, all before the first run.

Help pages are simulated until Effort 1 writes real ones. `help <topic>` returns an excerpt cut at
paragraph boundaries to at most 300 tokens, the `[help]` budget in `bench/budget.toml`. The
excerpt comes from the `docs/semantics.md` section below. `docs/semantics.md` never mentions six
index topics, so those come from the matching paragraph of `docs/language.md`, which the core
setups otherwise don't see. Every topic in either core's index has a row.

| topics | excerpt |
|---|---|
| targets, scale, profile, CLI | semantics 11. Targets, manifest and build |
| use, privacy | semantics 2. Parsing (the `use` row) and 4. Node identity |
| palette from, color details | semantics 6. Color |
| poly, star, path, dash, cap, join, jitter, style, seed | semantics 7. Paint and geometry defaults |
| skew, pivot, frames | semantics 3. Coordinates and frames |
| repeat, grid, outside | semantics 13. Clarifications from implementation |
| fx as/of/merge, fx details, finish | semantics 8. Effects |
| mask, slots, needs, overrides | semantics 9. Components, slots and overrides |
| rule, expect, checks | semantics 10. Checks |
| conic, blob, xor, mirror, feather, anchor, fields | the card's sentence or table row for the topic |

Freeze the excerpts in `bench/help_sim/<topic>.md` before the first run so every run sees the same
text.

## Tasks

The three constructions and five edits that span the edit kinds:

- `c01-ume-plum`, `c02-paper-lantern`, `c03-acorn-sprite`
- `v01-peach-soften-highlight`, `v06-lantern-taller-flame`, `s03-strawberry-add-dew-drop`,
  `m01-lantern-quieter-glow`, `x01-monk-sleepy`

Each run gets the task's `## Request` section only, not `## Do`, plus a fresh copy of
`examples/chibi`.

## Runs

- Three setups × eight tasks × at least 3 runs = 72 runs. Add runs to a task whose spread is
  wider than the gap it decides.
- One model, one harness and one tool set for every run: read a file or range, write a
  search-and-replace edit, `artscript build`, `artscript check`, look at a render, and `help` in the
  two core setups. Fix the model slug, the temperature and the turn limit (30 turns) in the run log.
- Interleave setups by task and run index, so drift in the model service hits every setup alike.
- Store each run's full transcript and its metrics as `bench/card_runs/<setup>/<task>/<run>/`.

## Measures

Count every token in o200k_base, and images with the `cost.py` default of `ceil(w*h/750)`.

- **Total tokens.** The prompt (card plus request) once per run, plus every help page, file read,
  command output and image the agent receives, plus every token the agent writes, including the
  turns it spends fixing its own errors. Prompt caching is ignored. The card is counted once per
  run, not once per turn, so the measure is the cheaper reading for `full`.
- **Round trips.** Every build, check, look and help call.
- **First-build success.** The first `artscript build` exits 0 and the task's acceptance block
  passes on that state.
- **Review score.** The judge procedure in `bench/REVIEW.md` on the final state, scored blind to
  the setup. A run's score is its lowest criterion. An edit that fails its acceptance block scores 1.

## Decision

Per setup and task, take the median over runs of each measure. Sum the medians of total tokens and
of round trips over the eight tasks, and average the median review scores.

Choose the smallest core for which, against `full`:

1. total tokens rise by no more than 5%,
2. round trips rise by no more than 5%, and
3. the mean review score does not drop.

If neither core passes, keep the full card and lower `[card] tokens` only by what an edit to it
saves. Report each setup's run count, the per-task min and max beside each median, and the first-
build success rate. If a setup passes or fails on a gap smaller than its spread, say so and add runs
before acting on it.

Once a setup is chosen, set `[card] tokens` in `bench/budget.toml` to its size rounded up to 50.
