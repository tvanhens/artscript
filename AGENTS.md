# ramose

`docs/language.md` and the topic files in `docs/topics/` are the author card. They describe the draft the code implements today. The design is still open. A better design can replace any rule, and it can break today's outputs.

## Code Review Rules

### The design is still open

Choose the better design. When that design breaks today's behavior, accept the break.

Flag a sentence in the card or a topic that this change leaves false. Choose the better behavior and fix that sentence.

A limitation in the draft is open. Accept a change that replaces it with a better behavior. A finding about a limitation names the better behavior. Without that name, leave the limitation out of the review.

A limit that drops or corrupts output and reports no error is a finding even when the card states it. Name the repair as reporting an error, or as doing the work. A stated prepass limit can be replaced. So can a cap, or a rule that there is one way to write something, when a better design needs a different rule.

Write a rule into the card or a topic when you want that behavior. Accidental behavior stays out of the card until you choose it.

A PR description records why its design choice was made. Replace an earlier choice when a better design needs it, and name the choice it replaces in the PR description. A show-me-your-work decision log stays local and out of git.

The only docs are the ones `ramose docs` prints. A design decision lands in the PR that implements it, updates what `ramose docs` prints in that same PR, and records its reasoning in the PR description. Do not add a document the CLI cannot print.

### Inputs are the author's own files

- ramose is a local authoring CLI. Its inputs are the author's own `.ramo` files and manifest, not untrusted uploads. Do not flag memory or CPU exhaustion that needs a pathological input, such as an extreme `fps`, `dur`, canvas size, or frame count. Do not ask for new per-resource caps, budgets shared across workers, or streaming rewrites to survive such inputs.
  Flag instead: a crash, hang, or out-of-memory exit on an input the size of the files in `examples/`, and any limit that silently truncates or corrupts output instead of reporting an error.

### Builds are deterministic

- The same source, manifest, and resolved backend (`cpu` or `gpu`) must produce byte-identical outputs on every build on one machine. GPU output may differ from CPU output and between machines. A language change may change those bytes. After that change, two builds of the same source, manifest, and backend still have to match each other. Flag anything that breaks this, such as a `rand` or `pick` evaluated without its site seed, iteration over an unordered collection, or an option for one output format that changes another format's bytes.

### Diagnostics

- Flag a failure that is lost, reported twice, or reported at the wrong file or line. Do not flag the wording of a fix hint, or which frame's notes a diagnostic that spans several frames keeps. A change to either of those is not a drift finding. The docs may keep the old wording.

### Visual goldens

- `catalog/golden/` holds locked images of the catalog fixtures in `catalog/art/`. Each fixture's header comment says what its image must show. The review job writes a sheet for each golden the PR adds, changes, or deletes, and lists them in `.claude-review/visual/index.md`. Look at every sheet.
- Flag a changed golden that the PR does not account for: no change in the PR draws it differently, and the PR description does not name it. An unexplained change is a regression until the author says otherwise.
- Flag a golden that contradicts its fixture's comment or the card, such as a child the comment says is clipped that spills past its parent. This holds for a new golden too.
- Flag a fixture comment that the PR rewrites to describe a wrong image.
- Flag a deleted golden whose feature no other fixture still draws, unless the PR removes the feature.
- Do not flag a change that the PR intends and that matches the fixture's comment.

## When the `visual` test fails

- `cargo test -p ramose --test visual` builds `catalog/` on the CPU backend and compares a PNG view of every output with its golden in `catalog/golden/`. A PNG output is its own view. A GIF view is a strip of its frames, eight to a row, with a gray gap between frames. An SVG view is the SVG rasterized by resvg.
- A view matches its golden when at most one pixel in 1000 changed. A pixel changed when one of its channels moved by more than 8. Skia rounds a little differently on each platform, so goldens blessed on macOS still match on Linux CI.
- A failure writes a `golden | actual | diff` sheet for each changed golden to `target/visual-diff/`. The diff dims the pixels that match and paints changed pixels red. CI uploads the sheets as the `visual-diff` artifact.
- Open every sheet. If a change is a regression, fix the code.
- If every change is intended, run `RAMOSE_BLESS=1 cargo test -p ramose --test visual`, commit `catalog/golden/`, and name each changed golden and the reason in the PR description. Bless rewrites a golden when any of its pixels changed, even when the check would pass, and removes goldens that no output makes. Open each changed golden before you commit it.
- To build part of the catalog, set `RAMOSE_VISUAL_ONLY` to an `--only` glob, such as `RAMOSE_VISUAL_ONLY='art/fx/**'`. A filtered run never removes goldens.

## Write a catalog fixture

A new language feature gets a fixture.

- Put one feature in one file at `catalog/art/<area>/<feature>.ramo`, with targets at `<area>/<feature>.<ext>`.
- Start the file with a comment that says what the image must show: the cases, their order, their colors, and what makes each one correct. The reviewer judges the golden against this comment, without the code.
- Show the cases of one feature side by side in one canvas, not in separate files. Label nothing. The comment names the cells by position.
- Keep goldens small. A still is at most 256 on its long side, and 128 to 192 is usual. A GIF is at most 96x96, with 8 fps or less and 12 frames or less.
- Add an `.svg` target only where the SVG writer draws the feature in its own way, such as gradients, masks, blends, and baked raster effects.
- Use the `p` palette in `catalog/lib/palette/swatch.ramo`. The default background is paper, so `bg:` is only needed for a transparent case.
- A fixture builds with no warnings and is formatted. Run `ramose fmt` on it.

## When `claude-review` fails

- Wait for the check with `gh pr checks <pr> --watch`. A review takes a few minutes.
- The blocking findings are in the PR's "Claude review" comment, with links to their threads. Read the unresolved threads for the detail.
- Fix the code, or reply on the thread with a concrete reason: the input or state that makes the finding wrong. Then push.
- Each push is re-reviewed. The reviewer resolves a thread when the code is fixed or it accepts your reply.
- To have a reply re-evaluated with no code change, run `gh run rerun <run-id> --failed`.
- The check says "did not run" when the review crashed or was skipped. Rerun it the same way.
- The check fails when the reviewer did not review every golden this PR changes. Rerun it the same way.
- Do not resolve threads by hand or edit `.github/claude-review/` to get a pass.

## Finishing a task

Open a PR, then babysit it until it merges. Watch the checks, fix every failure, and answer review threads as the `claude-review` section says. Rebase onto origin/main whenever it moves or a conflict appears. A PR that was green once and then fell behind main is not done.
