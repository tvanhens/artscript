# Edit and construction benchmark

Twenty art-director requests against `examples/chibi`, each with a reference solution and a
reference transcript of how a competent agent does it today. `cost.py` replays the transcripts
under a fixed cost model, so a change to the language or the CLI shows up as a change in tokens and
round trips.

```
uv run --with tiktoken python bench/cost.py            # per task and total, today's workflow
uv run --with tiktoken python bench/cost.py --json     # the same as JSON (bench/baseline/cost.json)
uv run --with tiktoken python bench/cost.py --variant cli   # compare steps.cli.jsonl against steps.jsonl
cargo test -p artscript --test bench                   # every solution applies and passes its acceptance
BENCH_TASKS=v01-peach-soften-highlight cargo test -p artscript --test bench   # just these ids
```

## A task

`bench/tasks/<id>/` holds three files. The id's letter is its kind: `v` value edit, `s` structural
edit, `m` multi-node edit, `x` variant, `c` construction.

`task.md` starts with a title line and a metadata block, then the request, then what a scorer
checks, then the acceptance block:

````
# Soften the peach's upper-left highlight

kind: value
piece: produce/momo_peach
reads: lib/face/parts.art

## Request

What the art director says.

## Do

The specific edit a scorer can check: which node, which keys, which values or ranges.

## Acceptance

```accept
changed produce/momo_peach.png 270,410 170x130
```
````

- `kind` is `value`, `structural`, `multi`, `variant` or `construction`.
- `piece` is the target path without extension. The transcript builds it with
  `--only '<piece>.*'` and looks at `<piece>.png`.
- `reads` (optional) lists files read before the edit beyond the ones the patch touches. A
  construction lists the references it imitates.
- Paths are relative to the project root (`examples/chibi`).

`artscript check` must exit 0 on every patched copy. Each line of the `accept` block adds one
more check:

| directive | passes when |
|---|---|
| `assert <file> <expr>` | `assert <expr>` appended to `<file>` fails `artscript check` before the patch and passes after it |
| `changed <png> <x,y> <WxH>` | at least 1% of the region's pixels change by more than 2 levels in some channel between the unpatched and patched renders |
| `unchanged <png> <x,y> <WxH>` | no pixel in the region moves at all, as for a variant's base render |
| `differs <png> <png> <x,y> <WxH>` | the same test between two outputs of the patched build, as for a variant against its base |
| `lines <file> <lo>..<hi>` | the patched file has between `lo` and `hi` lines |

Regions are in pixels of the 1x render, with the origin at the top left.

`solution.patch` is a unified diff with `a/` and `b/` prefixes relative to the project root, as
`git apply` reads it. Write it with `bench/author.py patch <id> <edited copy>`. It may add and
change sources but not delete them, because a transcript has no step that deletes a file.

`steps.jsonl` is the reference transcript, one step per line, generated with
`bench/author.py steps <id>`:

| step | meaning | cost |
|---|---|---|
| `{"read": "art/x.art"}` or `{"read": "art/x.art:40-60"}` | read a file or a line range | its text, in tokens read |
| `{"write": "<unified diff of one hunk>"}` | one search-and-replace edit | the hunk's old side plus its new side, in tokens written |
| `{"build": "<glob>"}` | `artscript build --only <glob>` | one round trip |
| `{"check": null}` | `artscript check` | one round trip |
| `{"look": "1024x1024", "path": "assets/x.png"}` | view a render | image tokens, one round trip |

A write's old side is the removed lines plus the fewest context lines that make them unique in the
file, which is what a search-and-replace tool call has to send. A new file is written whole.

## Today's workflow

`bench/author.py steps` writes the transcript a competent agent produces with today's tools:

1. Read every file the patch touches in full, plus the task's `reads`. A construction also looks
   at the render of its first reference piece.
2. One write per hunk.
3. Build the piece and look at its full render. A variant also looks at the variant's render.

Reference transcripts succeed on the first build, so they are a lower bound. Fix-up turns are what
the live experiment in `CARD_EXPERIMENT.md` measures.

A later effort that adds tools writes `steps.<variant>.jsonl` beside `steps.jsonl` and teaches
`cost.py` any new step kinds. `cost.py --variant <variant>` then reports both.

## Cost model

- Tokens are o200k_base, the encoding `scripts/tokens.py` uses.
- Image tokens default to `ceil(w*h/750)`, 1399 for a 1024x1024 render. `--image-formula` takes any
  Python expression in `w`, `h` and `ceil`.
- Total tokens are tokens read plus tokens written plus image tokens. The language card and the
  task prompt are the same in every variant, so they are left out; `scripts/card_budget.py` tracks
  the card.
