# artfuzz

A spike. `artfuzz` mutates the files in `examples/chibi` and checks properties that do not depend on how the evaluator works inside. It does not change any crate.

## Properties

| property | holds when |
|---|---|
| `fmt-idempotent` | `fmt` twice equals `fmt` once |
| `fmt-keeps-output` | the formatted file evaluates to the same scenes and diagnostics |
| `deterministic` | two evaluations and two renders of one input are identical |
| `comment-inert` | inserting a comment line changes no scene |
| `wrap-inert` | wrapping a draw or `play` in a block that always runs (`if true`, `if @n.w > -1`, `each x in [@n.w]`, `group` around a play) changes no scene. A wrapper that reads no node (`if true`, `each x in [0]`) must not introduce an error either. A wrapper that reads `@n` may, since the prepass cannot read the drawn scene |
| `scale-isolated` | adding `scale = [1, 2]` to the manifest leaves every 1x PNG and GIF byte-identical |
| `static-across-frames` | in a GIF, a patch with no time term is the same on every frame, and a linear key moves by equal steps |
| `diag-sane` | every diagnostic names a project file and a line inside it |
| `unique-paths` | no two nodes in one scene share a path |
| `last-play-wins` | an earlier `play` that a later `play` overrides on the same channel changes nothing, wherever it sits: top level, under an instance, inside a `group` or `layer`, or in `if true` or `each` |
| `gif-timing` | a GIF that samples without error has the frame count, frame times and total delay that `docs/semantics.md` §14 gives |

`cli_props.sh` checks three more at the CLI: `check` and `build` agree on error, assert, or ok; a cached rebuild prints the same report; and two fresh builds write the same bytes.

## Run

```
cargo build --release -p artfuzz -p artscript
./target/release/artfuzz --secs 600 --seed 1 --out target/fuzz-findings
./target/release/artfuzz repro tools/fuzz/seeds/*.art tools/fuzz/findings/*.art
tools/fuzz/cli_props.sh target/release/artscript target/release/artfuzz examples/chibi 60 1 target/fuzz-cli
```

The mutator also edits `@target` lines: `bg`, `fps`, `dur`, `clip`, `each` axes and `scale`.

Each new failure is bucketed, minimized by deleting lines, and written as a `.art` file with `# prop:` and `# entry:` headers. `repro` reruns those files. A run exits 1 when any failure matches no filed issue.

## Known issues

`known_issue` in `src/main.rs` maps a failure to the issue that tracks it, by the failure's shape:

| issue | failure |
|---|---|
| #39 | a comment line, or a `fmt` that moves lines, changes a `rand` in a `let` |
| #40 | adding scale 2 changes a 1x PNG |
| #41 | a `play` under a guard that reads `@node` is dropped |

A run counts these and does not fail on them. When you fix one of these bugs, delete its arm.

## Files

`seeds/` holds hand-written inputs for Codex findings on PR 13. Every seed must pass.

`findings/` holds minimal repros of the known issues. Each one has an `# issue:` header and must still fail. When a fix makes one pass, `repro` reports `FIXED`. Move the file to `seeds/`, delete its `# issue:` line, and close the issue.

## CI

The `fuzz` job in `.github/workflows/ci.yml` runs `ci.sh`. It replays `seeds/` and `findings/`, then runs four fixed seeds for 12 cases each, so a run is reproducible. Set `FUZZ_SEEDS` and `FUZZ_ITERS` to change that. On failure, the job uploads the minimized repros as the `fuzz-findings` artifact.
