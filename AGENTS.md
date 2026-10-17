# artscript

`docs/semantics.md` is the language spec. `docs/language.md` is the author-facing summary.

## Code Review Rules

### Inputs are the author's own files

- artscript is a local authoring CLI. Its inputs are the author's own `.art` files and manifest, not untrusted uploads. Do not flag memory or CPU exhaustion that needs a pathological input, such as an extreme `fps`, `dur`, canvas size, or frame count. Do not ask for new per-resource caps, budgets shared across workers, or streaming rewrites to survive such inputs.
  Flag instead: a crash, hang, or out-of-memory exit on an input the size of the files in `examples/`, and any limit that silently truncates or corrupts output instead of reporting an error.

### Builds are deterministic

- The same source and manifest must produce byte-identical outputs on every build. Flag anything that breaks this, such as a `rand` or `pick` evaluated without its site seed, iteration over an unordered collection, or an option for one output format that changes another format's bytes.

### The spec decides intent

- Flag code that disagrees with `docs/semantics.md`. A limitation that the spec states explicitly is intended behavior. Do not file it as an inline finding. If you think a stated limitation is wrong, say so once in the review summary.

### Diagnostics

- Flag a failure that is lost, reported twice, or reported at the wrong file or line. Do not flag the wording of a fix hint, or which frame's notes a diagnostic that spans several frames keeps.
