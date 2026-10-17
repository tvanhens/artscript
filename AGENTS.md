# artscript

`docs/semantics.md` and `docs/language.md` describe the draft the code implements today. The design is still open. A better design can replace any rule in either file, and it can break today's outputs.

## Code Review Rules

### The design is still open

Choose the better design. When that design breaks today's behavior, accept the break and update both docs in the same change.

Flag a change that leaves the code and the docs describing different behavior. Choose the better behavior and make them agree in that change.

A limitation in the draft is open. Accept a change that replaces it with a better behavior, and update the docs in that change. A finding about a limitation names the better behavior. Without that name, leave the limitation out of the review.

A limit that drops or corrupts output and reports no error is a finding even when the draft states it. Name the repair as reporting an error, or as doing the work. A stated prepass limit can be replaced. So can a cap, or a rule that there is one way to write something, when a better design needs a different rule.

Write a rule into the docs when you want that behavior. Accidental behavior stays out of the docs until you choose it.

When the two docs disagree, choose the better behavior and make them agree. A row in `docs/decisions.tsv` records an earlier choice. Replace that choice when a better design needs it, and append a new row. Leave the old row as it is.

### Inputs are the author's own files

- artscript is a local authoring CLI. Its inputs are the author's own `.art` files and manifest, not untrusted uploads. Do not flag memory or CPU exhaustion that needs a pathological input, such as an extreme `fps`, `dur`, canvas size, or frame count. Do not ask for new per-resource caps, budgets shared across workers, or streaming rewrites to survive such inputs.
  Flag instead: a crash, hang, or out-of-memory exit on an input the size of the files in `examples/`, and any limit that silently truncates or corrupts output instead of reporting an error.

### Builds are deterministic

- The same source and manifest must produce byte-identical outputs on every build. A language change may change those bytes. After that change, two builds of the same source and manifest still have to match each other. Flag anything that breaks this, such as a `rand` or `pick` evaluated without its site seed, iteration over an unordered collection, or an option for one output format that changes another format's bytes.

### Diagnostics

- Flag a failure that is lost, reported twice, or reported at the wrong file or line. Do not flag the wording of a fix hint, or which frame's notes a diagnostic that spans several frames keeps. A change to either of those is not a drift finding. The docs may keep the old wording.
