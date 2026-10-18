# brushes: brush declarations, SkSL bodies, the Mark, brush: on fills, stacks, seeds, std/brushes

A brush is an SkSL program that shades a fill. Its body sees a `Mark` with the pixel's position, its signed distance to the edge, and the paint it would have had, and returns a straight color plus coverage. The `brush:` paint key puts one or more brushes on a node.

```
use std/brushes { dry chip }
use house { wall }                                   # lib/house.art: let wall = dry(strength:.55 dir:-8)

layer back brush:wall                                # groups, layers and styles pass brush: down like fill:
  rect 512x300 at:256,362 fill:p.stone.2
  each i in 0..5
    rect 70x40 at:60+i*90,420 fill:p.stone.4 brush:chip(scale:1.4 strength:.7)
  blob 90 at:420,120 fill:p.stone.3 brush:[chip dry(strength:.3 dir:70)]
  circle 18 at:240,236 fill:p.moss.2 brush:none      # the node's own brush:none clears the inherited one
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

- `brush NAME k:v...` with exactly one `sksl """..."""` child. `brush` opens a declaration only as a line's first word. Everywhere else it is an ordinary name, so `brush:` is a key and `brush` may be a def parameter or a `let`. A brush is a value: it exports through `use` like a `def`, and `let wall = dry(strength:.55 dir:-8)` names a configured brush.
- Head keys are typed by their default. A key in the placement set (`dir`, `reach`, `on`) sets placement. Every other key declares a uniform the body reads by name: a number is `float`, a color is `float4` (straight rgba), a vector is `float2`. Any other default is E0363. A parameter cannot take the name of a `Mark` field, a helper, or start with `u_`, which belongs to the runtime (E0363). A body that uses a `u_` name is E0363.
- The body defines `float4 shade(Mark m)` and returns straight rgb and a coverage in 0..1. The frame multiplies the coverage by the paint's alpha, premultiplies, dithers, and snaps the result to 8-bit levels.
- The body is compiled when its file loads, so `artscript check` reports a broken brush without rendering, and a library brush is checked even when nothing uses it. An SkSL error is E0360 at the `.art` file, line, and column inside the `sksl` block. A broken parameter is reported at its key. An error outside the body, such as a missing `shade`, is reported at the `sksl` keyword. Every error in one body is reported, not only the first.

## The Mark

| field | meaning |
|---|---|
| `p` | the pixel in the node's local units |
| `dev` | the pixel in device pixels |
| `edge` | signed distance to the geometry's edge in local units, positive inside |
| `uv` | `p` relative to the geometry's center, rotated so x runs along `dir` |
| `s`, `u` | distance along `dir` from the near edge of the rotated bounds, in units and as 0..1 |
| `d`, `v` | offset across `dir` from the center, in units and as -1..1 |
| `len`, `width` | the size of the bounds along and across `dir` |
| `pressure`, `angle`, `tilt`, `velocity` | 1, `dir`, 0 and 1 on a fill. Strokes fill them later |
| `seed` | the stage's seed, 0..255 |
| `paint` | the straight rgba the pixel would have had: the fill at that pixel for the first stage, the previous stage's result after that |
| `px` | local units per device pixel |

Helpers: `hash12(p)`, `hash22(p)`, `vnoise(p)`, `simplex(p)`, `worley(p)`, `fbm(p, octaves)`, `rot(p, deg)`, `seeded(m)` (a per-stage offset to add to a noise position), `cover(d, m)` (signed distance in local units to an antialiased coverage, one pixel wide). The rest of SkSL is available: `mix`, `smoothstep`, `floor`, `fract`, and so on.

## Calling a brush

- `NAME(k:v...)` returns a new brush with those keys replaced; the declaration is unchanged. A key that is a declared parameter must match its kind. A placement key sets placement. An unknown key, a kind mismatch, or a positional argument is E0361, and the notes list the parameters and the placement keys. A parameter may be called again inside a def: `def stone tone:p.stone.3 brush:chip` with `rect size fill:tone brush:brush(scale:.5)`. A parameter whose default is a brush is brush-typed: the call's arguments are checked against the default's declaration whatever the instance passed, and the call is `none` when the instance passed `brush:none`.
- Placement: `dir:` is the brush direction in degrees. `reach:` is how far past the geometry the body may paint, in local units; the node's bounds and bake tiles grow by it, and ink past it is cut. `on:fill` or `on:stroke` narrows the brush to one paint slot.

## brush: on a node

- `brush:` is a paint key. It takes a brush, a list of brushes, or `none`. Like `fill:`, it is valid on every built-in, passes from `layer`, `group`, `inside`, `outside`, `mirror`, `repeat`, `grid`, and `style` to every shape in the block, and a shape's own key wins. It reaches the body of a def instance in the block: a def's shape with no `brush:` of its own takes the layer's. A def instance line takes `brush:` only when the def declares that parameter; otherwise it is E0321 like any unknown key. On a boolean operand it is E0321, like `fill:`.
- A bare word that is not a brush is E0364, and the fix names the nearest brush in scope. A number or any other kind is E0364. `brush:[]` is E0364; write `brush:none` to clear an inherited brush.
- A brush applies to every paint slot the node has. A fill gets an area mark. A stroke, including a tapered or width-list stroke, is not brushed yet: a brush that reaches a stroke slot is E0362, and the fix is `on:fill` or drawing the outline as its own node. A brush on the node's own line with no slot it can reach, such as `fill:none brush:dry`, is E0362. An inherited brush with no matching slot does nothing.
- `blur:` together with a brush is E0362. Blur the parent instead.
- Stacks: `brush:[a b]` runs the stages in list order on each slot. The first stage sees the fill's paint at that pixel as `m.paint`, so gradients and fields reach the body. Each later stage sees the previous stage's rgb as `m.paint`. Coverages multiply. Each stage has its own seed.
- A brushed node is raster-only. A `.svg` target with `profile:svg` bakes it to a PNG tile, and `profile:vector` reports E0209 with the feature `brush fill`.

## Seeds and determinism

- Each stage's seed comes from the node's key, the slot, and the stage's index, so a named node keeps its texture when other nodes change, and the `positional-seed` lint covers an unnamed one. `seed:` on the node pins every brush seed on it; `seed:` on an instance pins the brushes inside it.
- The shader sees 256 distinct `m.seed` values and 3599 distinct `seeded(m)` offsets. Two nodes in one scene can share a pattern. That is a deliberate trade: the hash inputs stay small enough to be exact on both backends.
- CPU and GPU builds each produce byte-identical outputs on every build. The two backends do not match each other bit for bit: the lattice hash, `rot`, the dither and the 8-bit snap are exact on both, but `simplex`, `vnoise`, `worley`, `fbm`, and SkSL's own `mix` and `smoothstep` are float arithmetic, so a GPU build can differ from a CPU build by a level or two at some pixels.

## std/brushes

`use std/brushes { dry chip }` imports the bundled library. `std/` names only bundled libraries: a project file in a folder named `std` is E0213, and `use std/nope` is E0210. `artscript std` lists the bundled libraries, and `artscript std brushes` prints the file, so `artscript std brushes > lib/brushes.art` forks it. The lints do not run on a bundled file.

- `dry strength:.5 scale:1 fade:0 dir:0`: coverage breaks into parallel streaks along `dir`; `fade` wears the paint out toward the far end.
- `chip strength:.6 scale:1 grit:.5`: a chipped, gritty edge. `scale` is the chip size, and the inside stays solid.
