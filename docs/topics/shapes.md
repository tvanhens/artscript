# shapes: the shape table, keys any node takes, paint keys (fill, stroke, taper, blur, jitter, brush, pressure, angle, tilt)

Shapes are centered on their local origin, and `at:` places that origin in the parent. A `curve` or `path` is the exception: its points are in its local frame, so `at:` moves the `0,0` of its points. Children are placed in the parent's local frame. Under `circle 40 at:100,80`, a child at `0,0` sits at the circle's center. Under a closed `curve`, a child at `0,0` sits at the `0,0` of the curve's points.

| shape | example |
|---|---|
| rect | `rect 40x20 r:4` (or `r:4,4,0,0` per corner; any other count is E0320) |
| circle, ellipse | `circle 12`, `ellipse 20,12` (radii) |
| poly, star | `poly 6 r:20 round:3`, `star 5 r:20 inner:.5` (at most 100000 points; a count that is not finite or is larger is E0320) |
| curve | `curve 0,0 20,-15 40,0`. Each positional is a point or a list of points, flattened in order, so `curve pts` and `curve pts 50,50 0,50` both work. Anything else is E0320. `closed` fills, `tension:.5`. `round:r` on a closed curve at `tension:0` |
| blob | `blob 30 seed:4 wobble:.2` organic closed shape |
| path | `path "M0 0 c10-20 30-20 40 0z"`. A path is closed only when every subpath ends in `Z` or `z`. Otherwise it is open like a curve without `closed` |
| copy | `copy @body at:-20,-8` reuses a named node's outline |
| booleans | `subtract`, `union`, `intersect`, `xor` with shape operands as children, first minus the rest. Every child is an operand, and the boolean reads only its geometry. `fill: stroke: cap: join: dash: taper: blur: a: blend:` or an `fx` line on an operand is E0321. To paint over the result, name the boolean and draw in `inside @name` |
| mirror | `mirror x` draws its block and the block flipped across x = 0 |
| repeat, grid | `repeat 8 rot:45`, `grid 4x3 gap:8` (`i`, `row`, `col` in the block). `cell:20x20` sets the cell size, which is otherwise the first copy's bounds. A copy reads references in its own cell. Without `cell:`, the first copy sets the pitch, so it cannot read a position from outside the grid (E0352) |

Any node takes `at: rot: scale: skew: pivot: a:` (opacity), `blend:` (`multiply screen overlay soft_light add erase` …), `z:behind|front|N`, `name:`, `style:`. `scale:` and `skew:` take a number or a vector, a skew stays strictly between -90 and 90 degrees, and the transform must be finite; anything else is E0320. Every key that takes a vector (`at:`, `pivot:`, `scale:`, `skew:`, `step:`, `cell:`, `taper:`, the `at:` of `radial` and `conic`, the `dir:` of `fx rim`) reports E0320 for a value that is not one, and does not fall back to its default. A key in canvas units only, such as `step:`, rejects a percent. A key that a built-in verb does not read is E0321, and the fix names the closest key it does read.

`at:` takes a vector, or an anchor such as `at:@body.neck`, which places the origin on that anchor. A reference to a node is E0320.

Paint keys work on a shape. On `layer`, `group`, `inside`, `outside`, `mirror`, `repeat`, and `grid`, they pass to every shape in the block, and a shape's own key wins.

Paint keys:

- `fill:<color|gradient|field>`.
- `stroke:<color>,<width>`. Strokes have round caps and joins. `stroke:3` alone uses `line(fill)`. A range is not a width. `stroke:p.ink,2..6` is an error, and the fix is `stroke:p.ink,[2 6]`. `cap:` and `join:` override, and `dash:4,2` or `dash:[4 2 3]` dashes. An odd dash list repeats once, as SVG does, in every format. A `dash:` that is not lengths is E0320.
- Tapered ink: `stroke:p.ink,[2 8 3] taper:.3` gives one width per point of a `curve`, or per on-curve vertex of a `path`, placed by chord length, with ends thinning over 30%. `taper:0,.6` sets the start and end separately. On a closed contour the closing chord counts and the list wraps: knot `i` gets width `i`, the stroke returns to the first width at the seam, and the seam has no cap. Each contour of a path is its own stroke, and `taper:` thins the ends of each one. A list with any other count spreads evenly by arc length over the whole path.
- `blur:6` softens a shape's edge. `blur:` together with `brush:` is E0362; blur the parent instead.
- `brush:dry`, `brush:[chip dry(strength:.3)]`, or `brush:none` shades the fill and the stroke with a brush, a stack of brushes, or clears an inherited one. A brushed stroke is a ribbon with round caps and joins and no dash: `dash:`, `cap:` and `join:` on the node's own line with it are E0362. A brushed node is raster-only. See `brushes`.
- `pressure:[.3 1 .5]`, `angle:45`, `tilt:[0 1]` are the channels a stroke brush reads. A number is constant, a list with one value per point sits on the points, any other list spreads evenly along the stroke. They need a brushed stroke: on the line of any other node they are E0362, and inherited they do nothing there. See `brushes`.
- `jitter:1.5,60` wobbles the outline like a hand-drawn edge. The pair is the amplitude and the wavelength.
