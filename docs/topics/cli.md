# cli: commands, preview crops and frame sheets, parameter overrides, exit codes

## Preview

```
ramose preview art/hero.ramo --target hero.png --crop head --pad 12 --zoom 4 --bg checker --out review/head.png
ramose preview art/hero.ramo --target hero.png --set view=geometry --set guides=true --out review/geometry.png
ramose preview art/hero.ramo --target hero.gif --frames 8 --columns 4 --out review/motion.png
```

`preview FILE --out PATH.png` writes one PNG inspection artifact and prints a JSON receipt. If the entry has several expanded outputs, `--target` must name exactly one output path, including its matrix values and scale suffix. No selector silently chooses the first output. `--backend auto|gpu|cpu` overrides the manifest backend.

| Option | Meaning |
|---|---|
| `--set name=value` | Bind a declared entry parameter. Repeat for several controls. See `values`. |
| `--crop x,y,w,h` | Rectangle in canvas units, with positive width and height. |
| `--crop node.path` | Named node's ink bounds, including its own clipping, mask, stroke, and ancestor placement and warps, excluding effects. An optional leading `@` is accepted. |
| `--pad N` | Nonnegative padding in canvas units, applied before raster rounding. |
| `--zoom N` | Positive integer enlargement of target pixels with nearest-neighbor sampling. Default 1. |
| `--bg checker\|light\|dark\|none\|#hex` | Presentation background. Default transparent. Light is white, dark is `#202020`, and checker uses eight-pixel squares before zoom. |
| `--time T` | One time in seconds. The default is the target's `t:`. |
| `--times T,...` | Explicit times in seconds, in the supplied order. |
| `--frames N` | Positive count of evenly spaced times from zero up to, but excluding, duration. Duration comes from `dur:` or the reached plays. |
| `--clip NAME` | Select an output whose target declares this clip. An incompatible `--target` is an error. |
| `--columns N` | Positive count of columns in the contact sheet. The default is one horizontal strip. |

Choose one of `--time`, `--times`, and `--frames`. Explicit times can include the loop endpoint. Named crops in a sheet use the union of bounds across its frames. Raster crop edges round outward after multiplication by the selected output scale. Regions beyond the target canvas are transparent before background presentation.

Preview renders the full target before cropping. `canvas.w`, matrix values, scale, occlusion, drawing order, and random choices keep their target context. A named crop retains surrounding artwork. Background presentation composites transparency without replacing an existing opaque target background. Zoom enlarges pixels rather than rerendering at a higher resolution. Parameter-driven drawings use normal clipping, effects, and drawing order. Naming unchanged artwork preserves its random seeds when diagnostic draws are added.

The destination must be outside the manifest output directory and `.ramose`. Preview does not write the production manifest or cache. Invalid selectors, bindings, missing crop nodes, and write failures exit with an error and do not emit a success receipt. A previous image at that destination does not indicate that a failed invocation succeeded.

A valid scene still produces a preview when an assertion fails or a lint fires. Diagnostics appear on stderr and in the receipt. The receipt's `exit_code` is 0 for a clean evaluation, 2 for assertion or lint errors, and 3 for warnings only. This lets authors inspect failed geometry without hiding the failed check. Compile errors prevent rendering and exit 1 without a receipt.

The receipt names the source, target path and line, matrix, scale, backend, resolved parameters per frame, times, duration, clip, canvas and pixel crop rectangles, zoom, background, layout, and generated paths. Parameter text preserves numeric precision. Resolved colors use `rgba(r g b a)` and percentages use `percent(fraction)` in the receipt. These descriptions are not CLI literal syntax. `parameter_altered` compares resolved values against defaults at each requested time. An altered view is diagnostic evidence. Inspect the normal output before claiming the shipping image is correct.

`build --set name=value` deliberately exports the selected parameterized variant to the normal production destinations. Its cache key includes parameter bindings, so a subsequent default build restores default pixels. `check --set name=value` validates equivalent bindings without writing images. Both accept repeated `--set`. Preview is the command for separate inspection artifacts.

`ramose build [--only 'glob']... [--allow-warnings] [--backend auto|gpu|cpu]`, `check [--json] [--only 'glob']... [FILE...]`, `fmt [--check] [FILE...]`, `explain <file>`, `init`, `docs [TOPIC...]`, `std [NAME]`. `--only` matches an entry file or an output path. `*` matches within one directory, `**` crosses directories, `?` matches one character, and `{a,b}` matches either alternative. Repeat `--only` to build the union of its globs. `build --only` leaves the outputs of every target it does not select. It deletes an output of a selected file only when no `@target` writes that output any more. A glob that matches no target is E0200, and nothing is built. A glob that matches only a file with errors reports those errors and no E0200. A brace without its partner prints usage and exits 1. With no files, `fmt` rewrites every `.ramo` file. `fmt --check` lists files that would change and writes nothing. `check` evaluates each still and samples each GIF once for each scale its outputs draw at, and it does not render. With `FILE` or `--only`, `check` covers only those entry files and the libraries they import, and its exit code reflects only them. `--only` takes the same globs as `build --only`. A `FILE` or glob that matches no target is E0200, and nothing is evaluated. A `FILE` with errors and no `@target` reports those errors. A `FILE` that does not exist or is not a `.ramo` file in the project prints usage and exits 1. `check --json` prints `{"diagnostics": [...]}`. Each diagnostic has `severity`, `code`, `file`, `span` (`line`, `col`), `node`, `targets`, `message`, `notes`, and `fix`. `targets` lists the first output path of each target whose evaluation raised it, and it is empty for a plan error and for a diagnostic about a file's own text, such as `raw-color`, `unused`, or a parse error. A diagnostic raised in a library has the note `in <entry>:<line>`, the innermost entry-file statement that reached it. `check` and `build` print a diagnostic that several targets raise alike once. Its header names one target as `[a.png]`, and several on a line under the header as `in 4 targets: a.png, b.png, c.png, … (+1)`. Targets that raise it with different notes or nodes print it separately, so two entry files that reach one library line print two diagnostics, each with its own `in <entry>:<line>` note. A diagnostic that several scales of one target raise alike prints once, and one that only some scales raise prints too. In one target, copies of a diagnostic that differ only in their node, such as one failure in every copy of a loop, print once as the first copy with the note `also in <node>, ...`. An unknown flag on any command prints usage and exits 1. Exit codes: 0 ok, 1 compile error or a `fmt --check` diff, 2 an assert failed or a lint set to `error` fired, 3 only warnings. `manifest` sets each lint's level. When the reader closes the output pipe, as `head` does, the command stops printing and exits with the same code.

`docs` prints the core card from `docs/language.md`, then a `## Topics` index with one line per file in `docs/topics/`: the topic name and the summary from the file's first line. `docs <topic> [topic...]` prints the core card, then each named topic file in the order given, separated by a blank line. Name every topic a task touches in one call. An unknown topic prints an error and the topic list to stderr and exits 1. `docs` runs from any directory, so it always matches the installed binary.

`std` lists the bundled libraries by their `use` path, one per line. `std <name>` prints that library's source, so `ramose std brushes > lib/brushes.ramo` forks it; `std/brushes` and `brushes` name the same file. An unknown name prints an error and the list to stderr and exits 1. `std` runs from any directory.
