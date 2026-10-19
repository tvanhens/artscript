# catalog

The catalog is a ramose project with one entry file per language feature. `crates/cli/tests/visual.rs` builds it on the CPU backend and compares a PNG view of every output with a golden in `golden/`. A PNG output is its own view. A GIF view is a strip of its frames, eight to a row, with a gray gap between frames. An SVG view is the SVG rasterized by resvg.

A view matches its golden when at most one pixel in 1000 changed, where a pixel changed when one of its channels moved by more than 8. Skia rounds a little differently on each platform, so goldens blessed on macOS still match on Linux CI. A change you can see moves far more pixels than that.

A golden is locked once a reviewer has judged it correct. When a change alters a golden, the test fails until you bless the new view, and the PR reviewer then compares the old and new golden.

## Run the check

```
cargo test -p ramose --test visual
```

A failure lists each golden that changed and writes a sheet to `target/visual-diff/<golden>`. The sheet shows the golden, the new view, and a diff, from left to right. The diff dims the pixels that match and paints changed pixels red. On CI the sheets are uploaded as the `visual-diff` artifact.

## Bless new goldens

```
RAMOSE_BLESS=1 cargo test -p ramose --test visual
```

Bless rewrites a golden when any of its pixels changed, even when the check would pass, and it removes goldens that no output makes. Rounding noise below the tolerance never rewrites a golden. Open each changed golden before you commit it. To build part of the catalog, set `RAMOSE_VISUAL_ONLY` to an `--only` glob, such as `RAMOSE_VISUAL_ONLY='art/fx/**'`. A filtered run never removes goldens.

## Write a fixture

- Put one feature in one file at `art/<area>/<feature>.ramo`, with targets at `<area>/<feature>.<ext>`.
- Start the file with a comment that says what the image must show: the cases, their order, their colors, and what makes each one correct. The reviewer judges the golden against this comment, without the code.
- Show the cases of one feature side by side in one canvas, not in separate files. Label nothing. The comment names the cells by position.
- Keep goldens small. A still is at most 256 on its long side, and 128 to 192 is usual. A GIF is at most 96x96, with 8 fps or less and 12 frames or less.
- Add an `.svg` target only where the SVG writer draws the feature in its own way, such as gradients, masks, blends, and baked raster effects.
- Use the `p` palette in `lib/palette/swatch.ramo`. The default background is paper, so `bg:` is only needed for a transparent case.
- A fixture builds with no warnings and is formatted. Run `ramose fmt` on it.
