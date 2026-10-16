# Art review gate

Any change that moves pixels in `examples/chibi` ships only after this review passes. The MVP
shipped through it (`docs/decisions.tsv`, the `art` and `C` rows from 05:02 to 07:55 on
2026-10-16), and it is the ship gate for later efforts too: a token saving that makes the art worse
is not a saving.

## Why a judge and not a threshold

A mean pixel-difference cap missed deleted fine detail twice during the MVP. The strawberry lost
its seeds, and the lantern lost its stone speckle and moss drape, while the mean barely moved
(the 04:52 row). Fine texture covers few pixels, so a mean can't see it go. The same holds for the
peach's fuzz. In commit a16149d, 0.33% of pixels moved and the mean channel difference was 0.25
levels, yet the 4x crop shows a comb of hairs turned into a fine fringe. A judge looking at crops
catches this, and a number does not. Pixel checks such as the `changed` lines in
`bench/tasks/*/task.md` prove that an edit landed. They never prove that it looks right.

## Procedure

1. **Render.** Run `scripts/review_crops.sh --before <base ref> <piece>...` for every piece the
   change touches, plus every piece that uses a changed library (`grep -l 'use face/parts'
   examples/chibi/art -r`). It builds each piece from clean copies with no cache, once at the base
   ref with artscript built from that ref and twice from the working tree with artscript built
   from the working tree, so an engine change shows up even when no `.art` file moved. It writes
   the following to `/tmp/artscript-review`:
   - `<piece>_light.png` and `<piece>_dark.png`: before on the left, after on the right, at full
     size on white and on near-black.
   - `<piece>_diff.png`: where the pixels moved, amplified 8x.
   - `<piece>_crop<n>_<x>_<y>.png`: 4x crops, before above after, light and dark. The crops cover
     the regions you pass with `--crop piece:x,y,WxH` plus the three tiles that changed most. Pass
     crops for fine detail the change could have deleted, even where nothing changed: fuzz, grain,
     speckle, seeds, lashes and glints.
   - `report.txt`: whether the two clean builds are byte-identical, and the changed share per piece.
2. **Determinism.** Every piece must report `two clean builds are byte-identical`. Stop if one
   differs. The MVP found a stale cache hiding an A/B this way (the 07:48 row), so the cache is
   always cleared.
3. **Judge.** Hand the sheets and crops to an independent judge, meaning a fresh agent that did not
   write the change. Give it the rubric, the images, and the request the change answers. Withhold
   the author's opinion of the result.
4. **Score.** The judge scores each piece 1 to 5 on each of the five criteria. A piece passes when
   every criterion scores 4 or higher. The judge cites a crop or a region for every score below 5.
5. **Fix and rejudge.** A failing piece goes back to a fixer with the judge's report. The rejudge is
   a fresh judge, not the one that failed it. The MVP took six judges to pass all six pieces.
6. **Confirm.** The coordinator looks at the worst two crops itself before accepting a pass or a
   fail (the 05:02 row), and at the full-size sheets before sending a fix to the judge. At 05:29
   that look sent a peach fix back that had turned the silhouette pale and blurry. The judge can
   overrule the coordinator, too. At 06:04 the second judge's measured crops reversed two of the
   coordinator's calls.

## Rubric

The MVP scored against the chibi style rubric in `../chibi/.cursor/skills/chibi-art/style.md`, a
sibling checkout that is not in this repository. It has six criteria, and `rig` is left out because
artscript exports flattened targets (the 03:20 row), which leaves five. The log names only two of
them, **rendering** and **craft**. Read the other three from that file when it is present. When it
is absent, record that in the review, have the judge score rendering and craft, and score three
criteria the log's failures were about. Label those three as stand-ins in the report:

- **silhouette**: the outline reads at thumbnail size, and no part is lost against light or dark.
- **light and color**: light comes from the upper left, shadows are cool and highlights warm, and
  the palette fits the set.
- **set consistency**: the piece sits beside the other chibi pieces in line weight, finish and face
  style.

## Record

Log each review with `scripts/log.sh docs/decisions.tsv`. Name the judge, every piece's five
scores, the path of the judge's report, and the worst crop.
