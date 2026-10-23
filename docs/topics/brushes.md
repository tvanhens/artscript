# brushes: brush declarations, the body language, the sample m, brush: on fills, strokes and outlines, channels, stacks, seeds, std/brushes

A brush shades a fill, a stroke, or the ring of an `fx outline`. Its body is a few lines of ramose over the sample `m`: the pixel's position, its signed distance to the edge, where it sits along and across the stroke, the paint it would have had, and what is already drawn under it. The body gives a straight color and a coverage, and ramose compiles it to a shader. The `brush:` paint key puts one or more brushes on a node.

```
use std/brushes { dry chip bristle }
use house { wall }                                   # lib/house.ramo: let wall = dry(strength:.55 dir:-8)

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
  let n = fbm(m.uv/(18*scale) oct:3)
  out lerp(m.paint.rgb tint.rgb strength*n*.4) cover:cover(m.edge+(n-.5)*rag)
```

- `brush NAME k:v...` with an indented body of `let`, `fn`, and `out` lines. `brush` opens a declaration only as a line's first word. Everywhere else it is an ordinary name, so `brush:` is a key and `brush` may be a def parameter or a `let`. A def cannot be named `brush` (E0105), since no instance line of it could be written. A brush is a value: it exports through `use` like a `def`, and `let wall = dry(strength:.55 dir:-8)` names a configured brush.
- Head keys are typed by the form of their default (see `components`, Parameter types). A key in the placement set (`dir`, `reach`, `on`) sets placement and is not a parameter: `scuff` reads its edge offset from `rag`, and `reach:3` lets it paint past the edge. Every other key declares a parameter the body reads by name: a number is a number, a color is a color (straight rgba), a vector is a vector of two or three components. Any other type is E0363 at the key, and a ramp is E0363 when the file loads. A parameter cannot be named `m`, after a field of `m`, or after a builtin; each is E0363 at its key. Words of the shader backend are free: `half:2` and `float:4` are ordinary parameters.
- The body is checked when its file loads, so `ramose check` reports a broken brush without rendering, and a library brush is checked even when nothing uses it. Every line of a body is checked, so one check reports the fault on each line that has one, not only the first. A helper that no line calls is still checked for the faults that need no argument types, such as an unknown name, an unknown field of `m`, or a function a body cannot call, and they are reported once at its `fn` line. A fault in the body is E0360 at its `let`, `fn`, or `out` line, or at the `cover:` key when it is in the coverage. A body with no `out` line is E0360 at the `brush` line. A body that still holds an `sksl """..."""` block from before this language is one E0360 at the `sksl` keyword, and nothing else is reported for it.

## The body

A body is a straight run of lines. Each line sees the parameters, `m`, the `let`s above it, and every helper of the body, above or below it.

- `let NAME = EXPR` binds a value. A `let` cannot be named `m`, after a field of `m`, after a builtin, or after an earlier `let`, a parameter, or a helper.
- `fn NAME(a b) = EXPR` declares a helper: it sees its own parameters, `m`, the brush's parameters, and every helper of the body, and not the body's `let`s; pass a `let` in as an argument. A helper cannot recurse. A helper's parameter cannot be named after a field of `m`: pass the field in, as in `f(m.edge)`.
- `out RGB cover:NUMBER` is the last line, exactly one per body. `RGB` is a three-component vector of straight rgb, such as `m.paint.rgb`; a color is E0360, and the fix says to write `.rgb`. `cover:` is the coverage in 0..1 and defaults to `cover(m.edge)`, the antialiased edge of the slot, so `out m.paint.rgb` draws the plain paint. The frame multiplies the coverage by the paint's alpha, premultiplies, dithers, and snaps the result to 8-bit levels.
- A body may call a top-level `fn` declared above the brush, either in its file or imported by a `use` line, as a `let` may. A fn named by a `use` line below the brush is unknown to the body. The fn sees only its parameters: not `m`, not the brush's parameters, not the file's palette or `let`s. A fault in the fn that does not depend on the arguments, such as a name it cannot read, is reported once at the fn line for the whole build. A fn that calls itself is E0325 at each fn on the cycle, as everywhere, and a body that calls it reports nothing more. A type fault is reported at the call, with a note naming the fn and its line, since another call with other types may be sound. A builtin wins over a fn of the same name.

Values: a number, a vector of two or three components, a color (straight rgba, only from a parameter), and a bool. Expressions are the values syntax (see `values`): `+ - * /` elementwise on vectors with a number scaling a vector, `== != < <= > >=`, `and or not`, `if c then a else b`, and tuples: `(x,y)` and `(x,y,z)` build vectors from numbers, and `(v,z)` extends a two-component vector. A vector reads `.x`, `.y`, `.z` and, on three components, `.xy`; a color reads `.r`, `.g`, `.b`, `.a`, and `.rgb`. What a sample has no use for is E0360 with a fix: a color literal (declare it in the head and read `tint.rgb`), a percent, a string, a list outside a reduction, an `@` reference, and `t`.

### Builtins

Each has its meaning from `values`, and runs once per pixel.

- `abs`, `floor`, `round`, `fract`, `sqrt`, `sin`, `cos`, `exp`: a number or a vector, elementwise. `sin` and `cos` take degrees. `sqrt` of a negative number is 0.
- `min`, `max`, `clamp`, `lerp`, `step`, `smoothstep`, `pow`, `mod`: numbers and vectors; a number beside a vector applies to every component. `lerp(a b k)` mixes; on two colors it is E0360, and the fix says to mix `.rgb`. `mod(a b)` is floored.
- `atan2(y x)` in degrees, `rad(deg)` to radians, `dot(a b)`, `length(v)`, `normalize(v)`, and `rot(p deg)`, a two-component vector turned by `deg`.
- `cover(d)` turns a signed distance in local units, positive inside, into an antialiased coverage one pixel wide. `cover(m.edge)` is the slot's own edge, and `cover(m.edge+3)` grows it by three units (with `reach:` to let it paint there).
- Noise: `hash(p)`, `hash2(p)`, `value(p)`, `simplex(p)`, `worley(p)`, and `fbm(p oct:4)` sample a two-component point. `hash` and `hash2` are white, one or two numbers per lattice cell; the others are smooth. Each noise is seeded by its stage: two stages at the same point see different patterns, and a named node keeps its pattern when other nodes change. `salt:k` on a noise gives an independent pattern from the same point, for a second texture that must not line up with the first. `seed:n` on a noise fixes its pattern for every stage, and `seed:0` is the unseeded lattice. `oct` is 1 to 8. `salt` and `seed` take whole constants; anything else is E0360.
- Reductions: `min`, `max`, and `sum` of a comprehension, `max([each x in -1..2 -> f(x)])`, run a loop in the shader. The bounds are whole constants, the comprehensions may nest, and the product of their trip counts is at most 256 items; a variable bound or a longer loop is E0360. They also take a literal list, `max([a b c])`.

## The sample

`m` is one struct for every slot. On a fill the frame is the geometry's bounds rotated into `dir`; on a stroke, and on an outline's ring (see Outlines), it is the ribbon, and `s` and `d` are measured to the point of the stroke's spine whose ink covers the pixel deepest, which is the nearest point wherever the spine does not pass near itself.

| field | on a fill | on a stroke |
|---|---|---|
| `position` | the pixel in the node's local units, as a three-component vector: `m.position.xy` is the pixel and `.z` is 0 | the same |
| `normal` | the surface normal, `(0,0,1)` until a brush can shade a volume | the same |
| `edge` | signed distance to the geometry's edge in local units, positive inside | the half width at `s` minus the distance to the spine, positive inside; past an open end it rounds the cap |
| `edge_normal` | the unit vector out of the shape at the nearest edge, in the node's local frame: the gradient of `-m.edge` with respect to `m.position.xy`. It is `(0,0)` where the edge has no single direction, such as on a medial axis | the unit vector from the nearest point of the spine to the pixel, in the node's local frame, so it points out of the stroke. A taper's slope does not tilt it. It is `(0,0)` on the spine and past `reach:` |
| `uv` | `position.xy` relative to the geometry's center, rotated so x runs along `dir` | `(s, d)` |
| `size` | the bounds along and across `dir`, so `m.size.x` is the length and `m.size.y` the width | the contour's arc length, and twice the half width at `s` |
| `s` | distance along `dir` from the near edge of the rotated bounds; `m.s/m.size.x` runs 0..1 | arc length along the spine to that point; past an open end it keeps running below 0 or above the length |
| `d` | offset across `dir` from the center; `m.d/(m.size.y/2)` runs -1..1 | offset across the spine, positive on the right of travel |
| `pressure`, `angle`, `tilt`, `velocity` | 1, `dir`, 0 and 1 | the node's channels at `s` |
| `seed` | the stage's seed, 0..255 | the same |
| `paint` | the straight rgba the pixel would have had: the fill at that pixel for the first stage, the previous stage's result after that | the same, from the stroke's paint |
| `below` | the straight rgba already drawn under the pixel in the node's group, as the backdrop a `blend:` on the node would composite with; see The backdrop | the same, with the node's own fill included |
| `px` | local units per device pixel | the same |
| `light` | reserved for the scene light; reading it is E0360 until that lands | the same |

`edge_normal` and `edge` together shade a form. This brush lights a bevel from the upper left. It mixes the paint toward white where the edge faces `toward` and toward black where it faces away, within `depth` units of the edge:

```
brush bevel toward:-1,-1 depth:7 strength:.7
  let k = dot(m.edge_normal normalize(toward))
  let band = 1-smoothstep(0 depth m.edge)
  let tint = if k > 0 then 1,1,1 else 0,0,0
  out lerp(m.paint.rgb tint abs(k)*band*strength)
```

`edge_normal` is in the node's local frame, as `position` is, so `toward` turns with a rotated node. On a fill it comes from the same device-pixel field as `edge`, so it follows the pixel grid: a straight edge is exact, and a curve is as smooth as its raster. A pixel on a medial axis gets `(0,0)`.

## The backdrop

`m.below` is the straight rgba already drawn under the pixel in the node's group, at 8-bit levels: the backdrop a `blend:` on the node would composite with. This brush smudges a shape into its neighbors, so a limb tucked under a body loses its hard edge:

```
brush smudge k:.5
  out lerp(m.paint.rgb m.below.rgb m.below.a*k)
```

- A group starts from nothing. A `layer`, a def instance, a stamp, and a `mask` block each open one, so a def draws the same pixels wherever it is called, and the first node in a `layer` sees a transparent backdrop. A shape with children or stamps, and any node with `a:`, `blend:`, `fx` or `mask`, also opens a group, but that group starts with the node's own fill and stroke. So a child's `m.below` is its parent's paint, the stamps, and the earlier siblings, and nothing from under the parent. At the top of a file the group is the artboard: `m.below` is the paper, or nothing when the target has no `bg:`, and what earlier nodes drew. A plain `group`, `inside` and `outside` open no group.
- The fill draws before the stroke, so a stroke's `m.below` includes the node's own fill. Every stage of a stack sees the same `m.below`, the backdrop under the node; the stage before it is `m.paint`.
- The paint of a node whose brush reads `m.below` is computed over the real backdrop and is still the node's own content: `a:`, `blend:`, `fx`, a mask and a clip apply to it as to any node, and its children blend with it and not with what is under the node, as before.
- The coverage that children clip to and that `fx outline` traces is computed over a transparent backdrop, so it does not depend on what is under the node.
- Two places draw a node offscreen, where there is no backdrop, and are errors instead of a wrong read. `fx outline` on a node whose brush reads `m.below` is E0362 at the `fx` line, since the outline rasterizes the node alone; outline a parent instead. A brush that reads `m.below` on an `fx outline ... brush:` line is E0362 at `brush:`, since the ring is drawn alone.
- An `.svg` target cannot bake such a node into a tile, which has no backdrop: `profile:svg` and `profile:vector` both report E0209 with the feature `brush below`. A `.png` or `.gif` target draws it.
- The backdrop is the deterministic composite so far, so a build stays byte-identical per backend. A body that reads `m.below` is compiled as a blender rather than a shader, and so is every other stage of its stack; `ramose check` compiles both forms, so a body that is sound in one is sound in the other.

## Calling a brush

- `NAME(k:v...)` returns a new brush with those keys replaced; the declaration is unchanged. A key that is a declared parameter must match its kind. A placement key sets placement. An unknown key, a kind mismatch, or a positional argument is E0361, and the notes list the parameters and the placement keys. A parameter may be called again inside a def: `def stone tone:p.stone.3 brush:chip` with `rect size fill:tone brush:brush(scale:.5)`. A parameter whose default is a brush is brush-typed: the call's arguments are checked against the default's declaration whatever the instance passed, and the call is `none` when the instance passed `brush:none`.
- Placement: `dir:` is the brush direction in degrees. `reach:` is how far past the geometry the body may paint, in local units; the node's bounds and bake tiles grow by it, and ink past it is cut. `on:fill` or `on:stroke` narrows the brush to one paint slot.

## brush: on a node

- `brush:` is a paint key. It takes a brush, a list of brushes, or `none`. Like `fill:`, it is valid on every built-in, passes from `layer`, `group`, `inside`, `outside`, `mirror`, `repeat`, `grid`, and `style` to every shape in the block, and a shape's own key wins. It reaches the body of a def instance in the block: a def's shape with no `brush:` of its own takes the layer's. A def instance line takes `brush:` only when the def declares that parameter; otherwise it is E0321 like any unknown key. On a boolean operand it is E0321, like `fill:`.
- A bare word that is not a brush is E0364, and the fix names the nearest brush in scope. A number or any other kind is E0364. `brush:[]` is E0364; write `brush:none` to clear an inherited brush.
- A brush applies to every paint slot the node has. A fill gets an area mark and a stroke gets a ribbon mark, so one brush on a node with both gives two marks, seeded apart. `on:fill` or `on:stroke` narrows it. A brush on the node's own line with no slot it can reach, such as `fill:none brush:dry`, is E0362, and so is each stage of a stack on that line that reaches no slot. An inherited brush with no matching slot does nothing.
- `blur:` together with a brush is E0362. Blur the parent instead. `dash:` together with a brush that reaches the stroke is E0362: a ribbon has no dash, and the dash is never dropped.
- Children of a node with a brushed fill clip to the coverage that fill leaves, so a child shows only where the fill's brush paints: the chips of `chip` and the streaks of `dry` cut it too. A pixel where the stages leave 40% coverage shows 40% of the child. The coverage is the product of the stage coverages and ignores the paint's alpha, so a translucent brushed fill keeps its children opaque. The coverage includes any `reach:` past the geometry, and is computed over a transparent backdrop (see The backdrop). A stroke brush does not widen it. Stamps are not children and stay unclipped. Children of a node whose fill is not brushed, a brushed stroke on a plain fill included, clip to the shape the node has without the brush: the geometry, and for a tapered or width-list stroke the stroke's tapered outline in place of the spine, joined with the fill when the node has one.
- A brush does not change the shape that `inside`, `copy`, `edge` and booleans see: it is the shape the node has without the brush, as just described, and not the brushed ink.
- `fx outline` on a node whose fill is brushed traces the coverage that fill leaves, where the stage coverages pass one half, so the ring follows the bites of `chip` and the streaks of `dry` and shows paper inside each. The traced coverage includes any `reach:` past the geometry and ignores the paint's alpha. A brushed stroke still adds the stroke's plain outline. This holds wherever the outline reaches a brushed fill: the node itself, a stamp, a child under a fill that shows nothing, or a mask drawn with shapes. Like `sides:`, such an outline is raster-only: `profile:svg` bakes the node to a PNG tile, and `profile:vector` reports E0209 with the feature `fx outline brush`. `fx outline` on a node whose own brush reads `m.below` is E0362 at the `fx` line (see The backdrop).
- Stacks: `brush:[a b]` runs the stages in list order on each slot. The first stage sees the fill's paint at that pixel as `m.paint`, so gradients and fields reach the body. Each later stage sees the previous stage's rgb as `m.paint`. Every stage sees the same `m.below`. Coverages multiply. Each stage has its own seed.
- A brushed node is raster-only. A `.svg` target with `profile:svg` bakes it to a PNG tile, and `profile:vector` reports E0209 with the feature `brush fill` or `brush stroke`. A node whose brush reads `m.below` cannot bake, since a tile has no backdrop: both profiles report E0209 with the feature `brush below`.

## Strokes

A brushed stroke is a ribbon. The shape's geometry is the spine, and every device pixel within the half width plus `reach:` of it takes one point of the spine: `m.s` is the arc length there, `m.d` the signed offset, `m.edge` the half width minus the distance, `m.edge_normal` the unit direction from the spine to the pixel. Where the spine crosses or doubles back on itself, the segment whose ink covers the pixel deepest wins: the one with the largest half width minus distance, the nearer one on a tie, then the earlier one. So the ribbon reads as one stroke and not as layers, and a thin tail that passes near a wide stem never carves it. At constant width this is the nearest segment. A ribbon has round caps and round joins, and the brush shapes its ends. `cap:` or `join:` on the node's own line, where that node's stroke is brushed, is E0362. An inherited `cap:` or `join:` does nothing there, and a stroke of one width with no brush and no `taper:` keeps both.

- Width: `stroke:p.ink,6` is a 6-unit ribbon. `stroke:p.ink,[2 8 3]` and `taper:.3` set the half width along the spine exactly as they shape an unbrushed tapered stroke, but the stroke stays a ribbon: the brush sees the true `s` and `d` along the spine, where an unbrushed tapered stroke fills its outline.
- Contours: a path with several contours gives one ribbon per contour, each with its own length in `m.size.x`, drawn in path order. Each contour is its own pen stroke, so `taper:` thins the start and end of every contour over its own length. A closed contour wraps: `s` runs from 0 at the contour's start around to the length, there is no cap, and the seam sits at the start, so a brush that rags its ends, such as `bristle`, rags the seam.
- Channels: `pressure:`, `angle:` (degrees) and `tilt:` are paint keys on any node with a stroke, and the brush reads them as `m.pressure`, `m.angle` and `m.tilt`. A number is constant along the path. The knots are a `curve`'s points and a `path`'s on-curve vertices, the ends of its segments. A list with one value per knot sits on the knots, placed by chord length within each contour, so on a path the values land on the vertices. On a closed contour the closing chord counts and the list wraps: knot `i` gets value `i`, and the stroke returns to the first value at the seam. Any other list spreads evenly by arc length over the whole path. Values interpolate linearly between knots. `m.velocity` is derived from the knots: they are taken as evenly timed, so a long chord between two knots is a fast segment, 1 is the mean, a knot moves at the mean of the chords on either side of it, and a shape with no knots, such as a `rect`, moves at 1. A channel that is not given is `pressure` 1, `angle` 0, `tilt` 0. The channels change nothing but what the brush reads: the width list stays the width.
- A channel key on the node's own line that nothing reads is E0362 ("has no effect because nothing on this node reads it") at the key. A brushed stroke reads every channel, and a stamp reads the channels its block or `spacing:` names (see `stamps`). An inherited channel key that nothing reads does nothing, like an inherited `cap:`, so `layer ink brush:bristle pressure:[.3 1 .5]` may hold a filled circle and a `style` with `pressure:` may serve a plain stroke. A channel value that is not a number or a list of numbers is E0320.
- The field is built on the CPU at device resolution for both backends, in strips over the ribbon's device bounds plus its reach, and the brush body runs once per pixel in those strips. The cost follows the ribbon's area, not the length of the path's point list. `m.s` is exact to a twentieth of a device pixel at any length along the stroke, because each strip counts it from the least arc length the strip holds, and `m.d` to about a tenth on a stroke 200 px wide. A strip that holds more than 65536 device px of one contour's length, such as a zigzag that crosses the same 64 rows hundreds of times, is a render error at the node; split the path.

## Outlines

`fx outline 4 brush:dry` shades the ring with the brush, so an outline can break up, dry out, and vary in weight like an inked line. `brush:` on an `fx outline` line takes what it takes on a node: a brush, a list of brushes, or `none`. The ring is drawn as a stroke: the traced area is filled solid, then each contour of it is a closed ribbon of constant half width `4` along the contour, and the stages run over it as over a brushed stroke, so the sample has its stroke meaning, with `pressure` 1, `angle` 0, `tilt` 0, and `velocity` 1. `m.paint` is the outline's color at full alpha, and the line's `a:` applies to the result.

- Each contour runs with the traced area on its right of travel: clockwise on screen around the outside, counter-clockwise around a hole. So `m.d > 0` is the half of the ribbon over the area, which the solid fill covers, and `m.d < 0` is the half that shows, where `m.edge_normal` points away from the area. A brush that rags an edge, such as `chip`, rags the ring's outer edge.
- `m.s` runs from 0 at the contour's start. When the outline traces one shape, the start is the shape's contour position (see `shapes`): a `rect`'s ring starts at its top-left corner and a `circle`'s at 3 o'clock, and a brush that rags its ends, such as `bristle`, rags the seam there. When it traces several areas, such as a stroke beside a fill or a parent with children, their union is traced and each contour starts at its own first point.
- A stage's `reach:` grows the ring: the node's bounds, its bake tiles, and the outline's reach grow by the widest reach on the sides the ring draws on. `sides:`, a `mask` under the line, and the erase inside translucent areas apply to a brushed ring as to a plain one.
- A stage narrowed to `on:fill` has no slot on a ring and is E0362 at `brush:`. So is a stage that reads `m.below`: the ring is drawn offscreen, with no backdrop.
- A brushed outline is raster-only: `profile:svg` bakes the node to a PNG tile, and `profile:vector` reports E0209 with the feature `brush outline`.

## Seeds and determinism

- Each stage's seed comes from the node's key, the slot (`fill` or `stroke`), and the stage's index, so a named node keeps its texture when other nodes change, and the `positional-seed` lint covers an unnamed one. A stage on an `fx outline` seeds from the node's key, the line's number among the node's `fx` lines, and the stage's index, and each line of a finish counts its own number under the `fx` line that expands it, so two brushed outlines on one node, or in one finish, seed apart. `seed:` on the node pins every brush seed on it, its outlines' included; `seed:` on an instance pins the brushes inside it.
- A body sees 256 distinct `m.seed` values, and its noise 3599 distinct lattice offsets, with `salt:k` mixing the stage seed and `k` into another offset in that range. Two nodes in one scene can share a pattern. That is a deliberate trade: the hash inputs stay small enough to be exact on both backends.
- CPU and GPU builds each produce byte-identical outputs on every build. The two backends do not match each other bit for bit: the lattice hash, `rot`, the dither and the 8-bit snap are exact on both, but `simplex`, `value`, `worley`, `fbm`, `lerp` and `smoothstep` are float arithmetic, so a GPU build can differ from a CPU build by a level or two at some pixels.

## std/brushes

`use std/brushes { dry chip bristle form }` imports the bundled library. `std/` names only bundled libraries: a project file under `std/` at the project root or at a lib root is E0213, and `use std/nope` is E0210. `ramose std` lists the bundled libraries, and `ramose std brushes` prints the file, so `ramose std brushes > lib/brushes.ramo` forks it. The lints do not run on a bundled file. `std/stamps` holds the bundled stamps; see `stamps`.

- `dry strength:.5 scale:1 fade:0 dir:0`: coverage breaks into parallel streaks along `dir` on a fill and along the ribbon on a stroke; `fade` wears the paint out toward the far end.
- `chip strength:.6 scale:1 grit:.5`: a chipped, gritty edge. `scale` is the chip size, and the inside stays solid. On a stroke thinner than about `strength * 18 * scale` the whole ribbon is edge.
- `bristle strength:.7 scale:1 gaps:.7 speed:.4`: for strokes. Bristle streaks along the ribbon, gaps where `pressure` is low (`gaps`) or `velocity` is high (`speed`), and ragged ends.
- `form sun:-.6,-.8 shadow:.8 lift:.4 rim:.5 strength:.5`: for fills. Shades the shape as a rounded volume lit from `sun`: a darker, cooler shade of the paint away from it (`shadow`), a paler, warmer one and a soft highlight toward it (`lift`), and reflected light on the shadow edge (`rim`). The light falls across the shape's bounds and rounds off near its edge, so a tall, wide, or irregular shape reads as one form. `strength` is a faint paint mottle.
