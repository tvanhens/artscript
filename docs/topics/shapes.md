# shapes: the shape table, keys any node takes, paint keys (fill, stroke, taper, blur, jitter)

Shapes are centered on their local origin, and `at:` places that origin in the parent.

| shape | example |
|---|---|
| rect | `rect 40x20 r:4` (or `r:4,4,0,0` per corner) |
| circle, ellipse | `circle 12`, `ellipse 20,12` (radii) |
| poly, star | `poly 6 r:20 round:3`, `star 5 r:20 inner:.5` |
| curve | `curve 0,0 20,-15 40,0`, or `curve pts` when `pts` is one list of points. `closed` fills, `tension:.5`. `round:r` on a closed curve at `tension:0` |
| blob | `blob 30 seed:4 wobble:.2` organic closed shape |
| path | `path "M0 0 c10-20 30-20 40 0z"` |
| copy | `copy @body at:-20,-8` reuses a named node's outline |
| booleans | `subtract`, `union`, `intersect`, `xor` with shape operands as children, first minus the rest |
| mirror | `mirror x` draws its block and the block flipped across x = 0 |
| repeat, grid | `repeat 8 rot:45`, `grid 4x3 gap:8` (`i`, `row`, `col` in the block) |

Any node takes `at: rot: scale: skew: pivot: a:` (opacity), `blend:` (`multiply screen overlay soft_light add erase` …), `z:behind|front|N`, `name:`, `style:`.

Paint keys:

- `fill:<color|gradient|field>`.
- `stroke:<color>,<width>`. Strokes have round caps and joins. `stroke:3` alone uses `line(fill)`. A range is not a width. `stroke:p.ink,2..6` is an error, and the fix is `stroke:p.ink,[2 6]`.
- Tapered ink: `stroke:p.ink,[2 8 3] taper:.3` on an open curve gives one width per point along its length, with ends thinning over 30%. `taper:0,.6` sets the start and end separately.
- `blur:6` softens a shape's edge.
- `jitter:1.5,60` wobbles the outline like a hand-drawn edge. The pair is the amplitude and the wavelength.
