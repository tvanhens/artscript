# brushes: brush declarations, SkSL bodies, the Mark, brush: on fills and strokes, channels, stacks, seeds, std/brushes

A brush is an SkSL program that shades a fill or a stroke. Its body sees a `Mark` with the pixel's position, its signed distance to the edge, where it sits along and across the stroke, and the paint it would have had, and returns a straight color plus coverage. The `brush:` paint key puts one or more brushes on a node.

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
brush scuff strength:.5 scale:1 tint:p.ink reach:3
  sksl """
  float4 shade(Mark m) {
    float n = fbm(m.uv / (18 * scale) + seeded(m), 3);
    return float4(mix(m.paint.rgb, tint.rgb, strength * n * .4), cover(m.edge + (n - .5) * reach, m));
  }
  """
```

- `brush NAME k:v...` with exactly one `sksl """..."""` child. `brush` opens a declaration only as a line's first word. Everywhere else it is an ordinary name, so `brush:` is a key and `brush` may be a def parameter or a `let`. A def cannot be named `brush` (E0105), since no instance line of it could be written. A brush is a value: it exports through `use` like a `def`, and `let wall = dry(strength:.55 dir:-8)` names a configured brush.
- Head keys are typed by their default. A key in the placement set (`dir`, `reach`, `on`) sets placement. Every other key declares a uniform the body reads by name: a number is `float`, a color is `float4` (straight rgba), a vector is `float2`. Any other default is E0363. A parameter cannot take the name of a `Mark` field, a helper, or start with `u_`, which belongs to the runtime (E0363). A body that uses a `u_` name outside a comment is E0363 at that name, and the body's other errors are still reported.
- The body defines `float4 shade(Mark m)` and returns straight rgb and a coverage in 0..1. The frame multiplies the coverage by the paint's alpha, premultiplies, dithers, and snaps the result to 8-bit levels.
- The body is compiled when its file loads, under the fill frame and the stroke frame, so `artscript check` reports a broken brush without rendering, and a library brush is checked even when nothing uses it. An SkSL error is E0360 at the `.art` file, line, and column inside the `sksl` block. A broken parameter is reported at its key. An error outside the body, such as a missing `shade`, is reported once at the `sksl` keyword; the errors it causes inside the frame are left out. Every error in one body is reported, not only the first.

## The Mark

One struct serves both slots. On a fill the frame is the geometry's bounds rotated into `dir`; on a stroke it is the ribbon, and `s` and `d` are measured to the nearest point of the stroke's spine.

| field | on a fill | on a stroke |
|---|---|---|
| `p` | the pixel in the node's local units | the same |
| `dev` | the pixel in device pixels | the same |
| `edge` | signed distance to the geometry's edge in local units, positive inside | the half width at `s` minus the distance to the spine, positive inside; past an open end it rounds the cap |
| `uv` | `p` relative to the geometry's center, rotated so x runs along `dir` | `(s, d)` |
| `s`, `u` | distance along `dir` from the near edge of the rotated bounds, in units and as 0..1 | arc length along the spine to the nearest point, in units and as 0..1 of `len`; past an open end it keeps running below 0 or above `len` |
| `d`, `v` | offset across `dir` from the center, in units and as -1..1 | offset across the spine, positive on the right of travel, in units and as -1..1 of the half width |
| `len`, `width` | the size of the bounds along and across `dir` | the contour's arc length, and twice the half width at `s` |
| `pressure`, `angle`, `tilt`, `velocity` | 1, `dir`, 0 and 1 | the node's channels at `s` |
| `seed` | the stage's seed, 0..255 | the same |
| `paint` | the straight rgba the pixel would have had: the fill at that pixel for the first stage, the previous stage's result after that | the same, from the stroke's paint |
| `px` | local units per device pixel | the same |

Helpers: `hash12(p)`, `hash22(p)`, `vnoise(p)`, `simplex(p)`, `worley(p)`, `fbm(p, octaves)`, `rot(p, deg)`, `seeded(m)` (a per-stage offset to add to a noise position), `cover(d, m)` (signed distance in local units to an antialiased coverage, one pixel wide). The rest of SkSL is available: `mix`, `smoothstep`, `floor`, `fract`, and so on.

## Calling a brush

- `NAME(k:v...)` returns a new brush with those keys replaced; the declaration is unchanged. A key that is a declared parameter must match its kind. A placement key sets placement. An unknown key, a kind mismatch, or a positional argument is E0361, and the notes list the parameters and the placement keys. A parameter may be called again inside a def: `def stone tone:p.stone.3 brush:chip` with `rect size fill:tone brush:brush(scale:.5)`. A parameter whose default is a brush is brush-typed: the call's arguments are checked against the default's declaration whatever the instance passed, and the call is `none` when the instance passed `brush:none`.
- Placement: `dir:` is the brush direction in degrees. `reach:` is how far past the geometry the body may paint, in local units; the node's bounds and bake tiles grow by it, and ink past it is cut. `on:fill` or `on:stroke` narrows the brush to one paint slot.

## brush: on a node

- `brush:` is a paint key. It takes a brush, a list of brushes, or `none`. Like `fill:`, it is valid on every built-in, passes from `layer`, `group`, `inside`, `outside`, `mirror`, `repeat`, `grid`, and `style` to every shape in the block, and a shape's own key wins. It reaches the body of a def instance in the block: a def's shape with no `brush:` of its own takes the layer's. A def instance line takes `brush:` only when the def declares that parameter; otherwise it is E0321 like any unknown key. On a boolean operand it is E0321, like `fill:`.
- A bare word that is not a brush is E0364, and the fix names the nearest brush in scope. A number or any other kind is E0364. `brush:[]` is E0364; write `brush:none` to clear an inherited brush.
- A brush applies to every paint slot the node has. A fill gets an area mark and a stroke gets a ribbon mark, so one brush on a node with both gives two marks, seeded apart. `on:fill` or `on:stroke` narrows it. A brush on the node's own line with no slot it can reach, such as `fill:none brush:dry`, is E0362, and so is each stage of a stack on that line that reaches no slot. An inherited brush with no matching slot does nothing.
- `blur:` together with a brush is E0362. Blur the parent instead. `dash:` together with a brush that reaches the stroke is E0362: a ribbon has no dash, and the dash is never dropped.
- Children of a brushed shape clip to its plain geometry, not to the brushed ink. A child that reaches the edge covers the brush's gaps and bites there, so brush the child as well or keep it inside the edge.
- Stacks: `brush:[a b]` runs the stages in list order on each slot. The first stage sees the fill's paint at that pixel as `m.paint`, so gradients and fields reach the body. Each later stage sees the previous stage's rgb as `m.paint`. Coverages multiply. Each stage has its own seed.
- A brushed node is raster-only. A `.svg` target with `profile:svg` bakes it to a PNG tile, and `profile:vector` reports E0209 with the feature `brush fill` or `brush stroke`.

## Strokes

A brushed stroke is a ribbon. The shape's geometry is the spine, and every device pixel within the half width plus `reach:` of it takes the nearest point of the spine: `m.s` is the arc length there, `m.d` the signed offset, `m.edge` the half width minus the distance. Where the spine crosses or doubles back on itself the nearest segment wins, so the ribbon reads as one stroke and not as layers. A ribbon has round caps and round joins, and the brush shapes its ends. `cap:` or `join:` on the node's own line, where that node's stroke is brushed, is E0362. An inherited `cap:` or `join:` does nothing there, and a stroke without a brush keeps both.

- Width: `stroke:p.ink,6` is a 6-unit ribbon. `stroke:p.ink,[2 8 3]` and `taper:.3` set the half width along the spine exactly as they shape an unbrushed tapered stroke, but the stroke stays a ribbon: the brush sees the true `s` and `d`, and the geometry is not flattened to an outline.
- Contours: a path with several contours gives one ribbon per contour, each with its own `len`, drawn in path order. A closed contour wraps: `s` runs from 0 at the contour's start around to `len`, there is no cap, and the seam sits at the start, so a brush that rags its ends, such as `bristle`, rags the seam.
- Channels: `pressure:`, `angle:` (degrees) and `tilt:` are paint keys on any node with a stroke, and the brush reads them as `m.pressure`, `m.angle` and `m.tilt`. A number is constant along the path. A list with one value per knot of a `curve` or `path` sits on the knots, placed by chord length. Any other list spreads evenly by arc length. Values interpolate linearly between knots. `m.velocity` is derived from the knots: they are taken as evenly timed, so a long chord between two knots is a fast segment, 1 is the mean, and a shape with no knots moves at 1. A channel that is not given is `pressure` 1, `angle` 0, `tilt` 0. The channels change nothing but what the brush reads: the width list stays the width.
- A channel key on a node whose stroke is not brushed, or that has no stroke, is E0362 ("has no effect without a brushed stroke"), at the key on the node's own line or at the statement with a note when it is inherited. A channel value that is not a number or a list of numbers is E0320.
- The field is built on the CPU at device resolution for both backends, in strips over the ribbon's device bounds plus its reach, and the brush body runs once per pixel in those strips. The cost follows the ribbon's area, not the length of the path's point list. `m.s` is exact to a twentieth of a device pixel on a 4000 px stroke, and `m.d` to about a tenth on a stroke 200 px wide.

## Seeds and determinism

- Each stage's seed comes from the node's key, the slot (`fill` or `stroke`), and the stage's index, so a named node keeps its texture when other nodes change, and the `positional-seed` lint covers an unnamed one. `seed:` on the node pins every brush seed on it; `seed:` on an instance pins the brushes inside it.
- The shader sees 256 distinct `m.seed` values and 3599 distinct `seeded(m)` offsets. Two nodes in one scene can share a pattern. That is a deliberate trade: the hash inputs stay small enough to be exact on both backends.
- CPU and GPU builds each produce byte-identical outputs on every build. The two backends do not match each other bit for bit: the lattice hash, `rot`, the dither and the 8-bit snap are exact on both, but `simplex`, `vnoise`, `worley`, `fbm`, and SkSL's own `mix` and `smoothstep` are float arithmetic, so a GPU build can differ from a CPU build by a level or two at some pixels.

## std/brushes

`use std/brushes { dry chip bristle }` imports the bundled library. `std/` names only bundled libraries: a project file under `std/` at the project root or at a lib root is E0213, and `use std/nope` is E0210. `artscript std` lists the bundled libraries, and `artscript std brushes` prints the file, so `artscript std brushes > lib/brushes.art` forks it. The lints do not run on a bundled file.

- `dry strength:.5 scale:1 fade:0 dir:0`: coverage breaks into parallel streaks along `dir` on a fill and along the ribbon on a stroke; `fade` wears the paint out toward the far end.
- `chip strength:.6 scale:1 grit:.5`: a chipped, gritty edge. `scale` is the chip size, and the inside stays solid. On a stroke thinner than about `strength * 18 * scale` the whole ribbon is edge.
- `bristle strength:.7 scale:1 gaps:.7 speed:.4`: for strokes. Bristle streaks along the ribbon, gaps where `pressure` is low (`gaps`) or `velocity` is high (`speed`), and ragged ends.
