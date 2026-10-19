# brushes: brush declarations, SkSL bodies, the Mark, brush: on fills, strokes and outlines, channels, stacks, seeds, std/brushes

A brush is an SkSL program that shades a fill, a stroke, or the ring of an `fx outline`. Its body sees a `Mark` with the pixel's position, its signed distance to the edge, where it sits along and across the stroke, and the paint it would have had, and returns a straight color plus coverage. The `brush:` paint key puts one or more brushes on a node.

```
use std/brushes { dry chip bristle }
use house { wall }                                   # lib/house.art: let wall = dry(strength:.55 dir:-8)

layer back brush:wall                                # groups, layers and styles pass brush: down like fill:
  rect 512x300 at:256,362 fill:p.stone.2
  each i in 0..5
    rect 70x40 at:60+i*90,420 fill:p.stone.4 brush:chip(scale:1.4 strength:.7)
  blob 90 at:420,120 fill:p.stone.3 brush:[chip dry(strength:.3 dir:70)]
  circle 18 at:240,236 fill:p.moss.2 brush:none      # the node's own brush:none clears the inherited one
curve 40,90 140,40 260,70 stroke:p.ink,[2 8 3] taper:.3 brush:bristle pressure:[.3 1 .5]
```

## Declaring a brush

```
brush scuff strength:.5 scale:1 tint:p.ink rag:3 reach:3
  sksl """
  float4 shade(Mark m) {
    float n = fbm(m.uv / (18 * scale) + seeded(m), 3);
    return float4(mix(m.paint.rgb, tint.rgb, strength * n * .4), cover(m.edge + (n - .5) * rag, m));
  }
  """
```

- `brush NAME k:v...` with exactly one `sksl """..."""` child. `brush` opens a declaration only as a line's first word. Everywhere else it is an ordinary name, so `brush:` is a key and `brush` may be a def parameter or a `let`. A def cannot be named `brush` (E0105), since no instance line of it could be written. A brush is a value: it exports through `use` like a `def`, and `let wall = dry(strength:.55 dir:-8)` names a configured brush.
- Head keys are typed by their default. A key in the placement set (`dir`, `reach`, `on`) sets placement and is not a uniform: `scuff` reads its edge offset from `rag`, and `reach:3` lets it paint past the edge. Every other key declares a uniform the body reads by name: a number is `float`, a color is `float4` (straight rgba), a vector is `float2`. Any other default is E0363. A parameter cannot take the name of a `Mark` field, a helper, or an SkSL keyword, type or builtin such as `flat`, `half` or `sin`. It cannot start with `u_`, which belongs to the runtime. Each such parameter is E0363 at its key. A body that uses a `u_` name outside a comment is E0363 at that name, and the body's other errors are still reported.
- The body defines `float4 shade(Mark m)` and returns straight rgb and a coverage in 0..1. The frame multiplies the coverage by the paint's alpha, premultiplies, dithers, and snaps the result to 8-bit levels.
- The body is compiled when its file loads, under the fill frame and the stroke frame, so `artscript check` reports a broken brush without rendering, and a library brush is checked even when nothing uses it. An SkSL error is E0360 at the `.art` file, line, and column inside the `sksl` block. A broken parameter is reported at its key. An error outside the body, such as a missing `shade`, is reported once at the `sksl` keyword; the errors it causes inside the frame are left out. Every error in one body is reported, not only the first.

## The Mark

One struct serves both slots. On a fill the frame is the geometry's bounds rotated into `dir`; on a stroke, and on an outline's ring (see Outlines), it is the ribbon, and `s` and `d` are measured to the point of the stroke's spine whose ink covers the pixel deepest, which is the nearest point wherever the spine does not pass near itself.

| field | on a fill | on a stroke |
|---|---|---|
| `p` | the pixel in the node's local units | the same |
| `dev` | the pixel in device pixels | the same |
| `edge` | signed distance to the geometry's edge in local units, positive inside | the half width at `s` minus the distance to the spine, positive inside; past an open end it rounds the cap |
| `normal` | the unit vector out of the shape at the nearest edge, in the node's local frame: the gradient of `-m.edge` with respect to `m.p`. It is `float2(0)` where the edge has no single direction, such as on a medial axis | the unit vector from the nearest point of the spine to the pixel, in the node's local frame, so it points out of the stroke. A taper's slope does not tilt it. It is `float2(0)` on the spine and past `reach:` |
| `uv` | `p` relative to the geometry's center, rotated so x runs along `dir` | `(s, d)` |
| `s`, `u` | distance along `dir` from the near edge of the rotated bounds, in units and as 0..1 | arc length along the spine to that point, in units and as 0..1 of `len`; past an open end it keeps running below 0 or above `len` |
| `d`, `v` | offset across `dir` from the center, in units and as -1..1 | offset across the spine, positive on the right of travel, in units and as -1..1 of the half width |
| `len`, `width` | the size of the bounds along and across `dir` | the contour's arc length, and twice the half width at `s` |
| `pressure`, `angle`, `tilt`, `velocity` | 1, `dir`, 0 and 1 | the node's channels at `s` |
| `seed` | the stage's seed, 0..255 | the same |
| `paint` | the straight rgba the pixel would have had: the fill at that pixel for the first stage, the previous stage's result after that | the same, from the stroke's paint |
| `px` | local units per device pixel | the same |

`normal` and `edge` together shade a form. This brush lights a bevel from the upper left. It mixes the paint toward white where the edge faces `toward` and toward black where it faces away, within `depth` units of the edge:

```
brush bevel toward:-1,-1 depth:7 strength:.7
  sksl """
  float4 shade(Mark m) {
    float k = dot(m.normal, normalize(toward));
    float band = 1 - smoothstep(0, depth, m.edge);
    float3 tint = k > 0 ? float3(1) : float3(0);
    return float4(mix(m.paint.rgb, tint, abs(k) * band * strength), cover(m.edge, m));
  }
  """
```

`normal` is in the node's local frame, as `p` is, so `toward` turns with a rotated node. On a fill it comes from the same device-pixel field as `edge`, so it follows the pixel grid: a straight edge is exact, and a curve is as smooth as its raster. A pixel on a medial axis gets `float2(0)`.

Helpers: `hash12(p)`, `hash22(p)`, `vnoise(p)`, `simplex(p)`, `worley(p)`, `fbm(p, octaves)`, `rot(p, deg)`, `seeded(m)` (a per-stage offset to add to a noise position), `cover(d, m)` (signed distance in local units to an antialiased coverage, one pixel wide). The rest of SkSL is available: `mix`, `smoothstep`, `floor`, `fract`, and so on.

## Calling a brush

- `NAME(k:v...)` returns a new brush with those keys replaced; the declaration is unchanged. A key that is a declared parameter must match its kind. A placement key sets placement. An unknown key, a kind mismatch, or a positional argument is E0361, and the notes list the parameters and the placement keys. A parameter may be called again inside a def: `def stone tone:p.stone.3 brush:chip` with `rect size fill:tone brush:brush(scale:.5)`. A parameter whose default is a brush is brush-typed: the call's arguments are checked against the default's declaration whatever the instance passed, and the call is `none` when the instance passed `brush:none`.
- Placement: `dir:` is the brush direction in degrees. `reach:` is how far past the geometry the body may paint, in local units; the node's bounds and bake tiles grow by it, and ink past it is cut. `on:fill` or `on:stroke` narrows the brush to one paint slot.

## brush: on a node

- `brush:` is a paint key. It takes a brush, a list of brushes, or `none`. Like `fill:`, it is valid on every built-in, passes from `layer`, `group`, `inside`, `outside`, `mirror`, `repeat`, `grid`, and `style` to every shape in the block, and a shape's own key wins. It reaches the body of a def instance in the block: a def's shape with no `brush:` of its own takes the layer's. A def instance line takes `brush:` only when the def declares that parameter; otherwise it is E0321 like any unknown key. On a boolean operand it is E0321, like `fill:`.
- A bare word that is not a brush is E0364, and the fix names the nearest brush in scope. A number or any other kind is E0364. `brush:[]` is E0364; write `brush:none` to clear an inherited brush.
- A brush applies to every paint slot the node has. A fill gets an area mark and a stroke gets a ribbon mark, so one brush on a node with both gives two marks, seeded apart. `on:fill` or `on:stroke` narrows it. A brush on the node's own line with no slot it can reach, such as `fill:none brush:dry`, is E0362, and so is each stage of a stack on that line that reaches no slot. An inherited brush with no matching slot does nothing.
- `blur:` together with a brush is E0362. Blur the parent instead. `dash:` together with a brush that reaches the stroke is E0362: a ribbon has no dash, and the dash is never dropped.
- Children of a node with a brushed fill clip to the coverage that fill leaves, so a child shows only where the fill's brush paints: the chips of `chip` and the streaks of `dry` cut it too. A pixel where the stages leave 40% coverage shows 40% of the child. The coverage is the product of the stage coverages and ignores the paint's alpha, so a translucent brushed fill keeps its children opaque. The coverage includes any `reach:` past the geometry. A stroke brush does not widen it. Stamps are not children and stay unclipped. Children of a node whose fill is not brushed, a brushed stroke on a plain fill included, clip to the shape the node has without the brush: the geometry, and for a tapered or width-list stroke the stroke's tapered outline in place of the spine, joined with the fill when the node has one.
- A brush does not change the shape that `inside`, `copy`, `edge` and booleans see: it is the shape the node has without the brush, as just described, and not the brushed ink.
- `fx outline` on a node whose fill is brushed traces the coverage that fill leaves, where the stage coverages pass one half, so the ring follows the bites of `chip` and the streaks of `dry` and shows paper inside each. The traced coverage includes any `reach:` past the geometry and ignores the paint's alpha. A brushed stroke still adds the stroke's plain outline. This holds wherever the outline reaches a brushed fill: the node itself, a stamp, a child under a fill that shows nothing, or a mask drawn with shapes. Like `sides:`, such an outline is raster-only: `profile:svg` bakes the node to a PNG tile, and `profile:vector` reports E0209 with the feature `fx outline brush`.
- Stacks: `brush:[a b]` runs the stages in list order on each slot. The first stage sees the fill's paint at that pixel as `m.paint`, so gradients and fields reach the body. Each later stage sees the previous stage's rgb as `m.paint`. Coverages multiply. Each stage has its own seed.
- A brushed node is raster-only. A `.svg` target with `profile:svg` bakes it to a PNG tile, and `profile:vector` reports E0209 with the feature `brush fill` or `brush stroke`.

## Strokes

A brushed stroke is a ribbon. The shape's geometry is the spine, and every device pixel within the half width plus `reach:` of it takes one point of the spine: `m.s` is the arc length there, `m.d` the signed offset, `m.edge` the half width minus the distance, `m.normal` the unit direction from the spine to the pixel. Where the spine crosses or doubles back on itself, the segment whose ink covers the pixel deepest wins: the one with the largest half width minus distance, the nearer one on a tie, then the earlier one. So the ribbon reads as one stroke and not as layers, and a thin tail that passes near a wide stem never carves it. At constant width this is the nearest segment. A ribbon has round caps and round joins, and the brush shapes its ends. `cap:` or `join:` on the node's own line, where that node's stroke is brushed, is E0362. An inherited `cap:` or `join:` does nothing there, and a stroke of one width with no brush and no `taper:` keeps both.

- Width: `stroke:p.ink,6` is a 6-unit ribbon. `stroke:p.ink,[2 8 3]` and `taper:.3` set the half width along the spine exactly as they shape an unbrushed tapered stroke, but the stroke stays a ribbon: the brush sees the true `s` and `d` along the spine, where an unbrushed tapered stroke fills its outline.
- Contours: a path with several contours gives one ribbon per contour, each with its own `len`, drawn in path order. Each contour is its own pen stroke, so `taper:` thins the start and end of every contour over its own length. A closed contour wraps: `s` runs from 0 at the contour's start around to `len`, there is no cap, and the seam sits at the start, so a brush that rags its ends, such as `bristle`, rags the seam.
- Channels: `pressure:`, `angle:` (degrees) and `tilt:` are paint keys on any node with a stroke, and the brush reads them as `m.pressure`, `m.angle` and `m.tilt`. A number is constant along the path. The knots are a `curve`'s points and a `path`'s on-curve vertices, the ends of its segments. A list with one value per knot sits on the knots, placed by chord length within each contour, so on a path the values land on the vertices. On a closed contour the closing chord counts and the list wraps: knot `i` gets value `i`, and the stroke returns to the first value at the seam. Any other list spreads evenly by arc length over the whole path. Values interpolate linearly between knots. `m.velocity` is derived from the knots: they are taken as evenly timed, so a long chord between two knots is a fast segment, 1 is the mean, a knot moves at the mean of the chords on either side of it, and a shape with no knots, such as a `rect`, moves at 1. A channel that is not given is `pressure` 1, `angle` 0, `tilt` 0. The channels change nothing but what the brush reads: the width list stays the width.
- A channel key on the node's own line that nothing reads is E0362 ("has no effect because nothing on this node reads it") at the key. A brushed stroke reads every channel, and a stamp reads the channels its block or `spacing:` names (see `stamps`). An inherited channel key that nothing reads does nothing, like an inherited `cap:`, so `layer ink brush:bristle pressure:[.3 1 .5]` may hold a filled circle and a `style` with `pressure:` may serve a plain stroke. A channel value that is not a number or a list of numbers is E0320.
- The field is built on the CPU at device resolution for both backends, in strips over the ribbon's device bounds plus its reach, and the brush body runs once per pixel in those strips. The cost follows the ribbon's area, not the length of the path's point list. `m.s` is exact to a twentieth of a device pixel at any length along the stroke, because each strip counts it from the least arc length the strip holds, and `m.d` to about a tenth on a stroke 200 px wide. A strip that holds more than 65536 device px of one contour's length, such as a zigzag that crosses the same 64 rows hundreds of times, is a render error at the node; split the path.

## Outlines

`fx outline 4 brush:dry` shades the ring with the brush, so an outline can break up, dry out, and vary in weight like an inked line. `brush:` on an `fx outline` line takes what it takes on a node: a brush, a list of brushes, or `none`. The ring is drawn as a stroke: the traced area is filled solid, then each contour of it is a closed ribbon of constant half width `4` along the contour, and the stages run over it as over a brushed stroke, so the Mark has its stroke meaning, with `pressure` 1, `angle` 0, `tilt` 0, and `velocity` 1. `m.paint` is the outline's color at full alpha, and the line's `a:` applies to the result.

- Each contour runs with the traced area on its right of travel: clockwise on screen around the outside, counter-clockwise around a hole. So `m.d > 0` is the half of the ribbon over the area, which the solid fill covers, and `m.d < 0` is the half that shows, where `m.normal` points away from the area. A brush that rags an edge, such as `chip`, rags the ring's outer edge.
- `m.s` runs from 0 at the contour's start. When the outline traces one shape, the start is the shape's contour position (see `shapes`): a `rect`'s ring starts at its top-left corner and a `circle`'s at 3 o'clock, and a brush that rags its ends, such as `bristle`, rags the seam there. When it traces several areas, such as a stroke beside a fill or a parent with children, their union is traced and each contour starts at its own first point.
- A stage's `reach:` grows the ring: the node's bounds, its bake tiles, and the outline's reach grow by the widest reach on the sides the ring draws on. `sides:`, a `mask` under the line, and the erase inside translucent areas apply to a brushed ring as to a plain one.
- A stage narrowed to `on:fill` has no slot on a ring and is E0362 at `brush:`.
- A brushed outline is raster-only: `profile:svg` bakes the node to a PNG tile, and `profile:vector` reports E0209 with the feature `brush outline`.

## Seeds and determinism

- Each stage's seed comes from the node's key, the slot (`fill` or `stroke`), and the stage's index, so a named node keeps its texture when other nodes change, and the `positional-seed` lint covers an unnamed one. A stage on an `fx outline` seeds from the node's key, the line's number among the node's `fx` lines, and the stage's index, and each line of a finish counts its own number under the `fx` line that expands it, so two brushed outlines on one node, or in one finish, seed apart. `seed:` on the node pins every brush seed on it, its outlines' included; `seed:` on an instance pins the brushes inside it.
- The shader sees 256 distinct `m.seed` values and 3599 distinct `seeded(m)` offsets. Two nodes in one scene can share a pattern. That is a deliberate trade: the hash inputs stay small enough to be exact on both backends.
- CPU and GPU builds each produce byte-identical outputs on every build. The two backends do not match each other bit for bit: the lattice hash, `rot`, the dither and the 8-bit snap are exact on both, but `simplex`, `vnoise`, `worley`, `fbm`, and SkSL's own `mix` and `smoothstep` are float arithmetic, so a GPU build can differ from a CPU build by a level or two at some pixels.

## std/brushes

`use std/brushes { dry chip bristle }` imports the bundled library. `std/` names only bundled libraries: a project file under `std/` at the project root or at a lib root is E0213, and `use std/nope` is E0210. `artscript std` lists the bundled libraries, and `artscript std brushes` prints the file, so `artscript std brushes > lib/brushes.art` forks it. The lints do not run on a bundled file. `std/stamps` holds the bundled stamps; see `stamps`.

- `dry strength:.5 scale:1 fade:0 dir:0`: coverage breaks into parallel streaks along `dir` on a fill and along the ribbon on a stroke; `fade` wears the paint out toward the far end.
- `chip strength:.6 scale:1 grit:.5`: a chipped, gritty edge. `scale` is the chip size, and the inside stays solid. On a stroke thinner than about `strength * 18 * scale` the whole ribbon is edge.
- `bristle strength:.7 scale:1 gaps:.7 speed:.4`: for strokes. Bristle streaks along the ribbon, gaps where `pressure` is low (`gaps`) or `velocity` is high (`speed`), and ragged ends.
