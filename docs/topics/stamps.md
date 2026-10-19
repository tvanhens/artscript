# stamps: stamp declarations, the block and its bound names, spacing and count, stamp: on a node, sets and references, seeds, std/stamps

A stamp is a block of drawing that a node lays along its geometry. The block is drawn once per placement, in a frame that sits on the path with x along the direction of travel, and it reads the stroke's channels there. The `stamp:` paint key puts one or more stamps on a node.

```
use std/brushes { bristle }
use std/stamps { dots leaves }

def tuft h:12 tint:p.grass.3
  each k in 0..3
    curve 0,0 (k-1)*2,-h*.6 (k-1)*4,-h stroke:tint,1.2 taper:0,.9 name:blade

# Upright tufts with a random lean: the frame follows the tangent, so rot:-tangent stands them up.
stamp tufts spacing:9 h:12
  tuft h*(.7+.6*rand(0 1)) at:0,rand(-1 1) rot:-tangent+rand(-10 10) name:tuft

curve 40,420 140,300 260,330 420,180 stroke:p.bark,3 brush:bristle pressure:[.9 .6 .7 .3] stamp:leaves name:vine
curve 60,60 200,40 340,70 stamp:dots(r:1.5 spacing:5 tint:p.ink) name:dotted      # no line under the dots
curve 0,300 120,260 260,290 400,250 512,280 stamp:tufts name:ridge
circle 60 at:256,256 fill:none stroke:p.ink,2 stamp:[dots(spacing:12) leaves(spacing:30 name:gold)] name:wreath
circle 4 at:@vine.leaves.6.leaf.tip fill:p.berry                                  # an anchor inside the seventh leaf
```

## Declaring a stamp

- `stamp NAME k:v...` with an indented block that takes what a def body takes: draws, `let`, `if`, `each`, and `fx` lines, which attach to every stamp. A `slot` or an `anchor` directly in the block is E0365. `stamp` opens a declaration only as a line's first word. Everywhere else it is an ordinary name, so `stamp:` is a key and `stamp` may be a def parameter or a `let`. A def cannot be named `stamp` (E0105). A stamp is a value: it exports through `use` like a `def`, and `let sparse = dots(spacing:12)` names a configured stamp.
- `spacing:` or `count:` places the stamps. A declaration has exactly one of them (E0365). Every other head key is a parameter, typed by its default like a def's, and the default is evaluated when the file loads. A parameter cannot take one of the bound names below, a `let` or `each` anywhere in the block cannot rebind one (E0365 for both), and `name:` is not a parameter (E0365): name a set at the call.
- The block runs once per placement. Its frame has the origin on the path, x along the direction of travel, and y to the right of it, the ribbon's `(s, d)` frame. A centered shape with no `at:` sits on the path, and a percent measures nothing, as in a def without `size`. `rot:-tangent` stands a stamp upright in the host's frame.
- The block sees these names, bound at each placement, after the stamp's parameters:

| name | value |
|---|---|
| `i` | the stamp's index in its set, from 0, counting along the path in contour order |
| `s`, `u` | arc length from the contour's start, in units and as 0..1 of `len` |
| `len` | the contour's arc length |
| `width` | the stroke's width at `s`, before any taper, or 0 without a stroke |
| `tangent` | the direction of travel in degrees of the host's frame, 0 along +x, 90 down |
| `pressure`, `angle`, `tilt`, `velocity` | the host's channels at `s`, as a brush reads them (see `brushes`) |
| `paint` | the stroke's paint at the point, or the fill's where the host has no stroke, or the default line color with neither. A gradient is sampled there |

- The block sees the paint keys of the layer or style the host sits in, with `stamp:` and the channel keys removed, so `layer garden brush:dry stamp:dots` reaches the shapes in the block without stamping them again. The host's own line's keys do not reach the block. An instance of a def that declares names needs `name:` on its line in the block, as in a def body (E0312).
- `rand` and `pick` in the block draw from the stamp's own seed, so every stamp gets its own numbers.
- An error in the block stops the set at that stamp, so one mistake is one diagnostic. A warning, such as a failing `expect`, is reported once per set at its line, and the set places every stamp.

## Placing

- `spacing:` is evaluated at every stamp, in the block's scope with the bound names. The first stamp sits at half the spacing read at `s = 0`. After a stamp at `s`, the next sits at `s` plus the spacing read there, so `spacing:8+8*pressure` packs stamps where the pressure is low. A percent is a percent of `width`, so `spacing:150%` on `stroke:p.ink,[2 8 3]` follows the stroke. A spacing that is not a positive finite number is E0368 at the spacing expression, with the stamp's path in a note. A percent on a node without a stroke is one, and its note says so.
- On a closed contour a constant spacing, one that reads no bound name and draws no random number, is spread at one even pitch: `round(len / spacing)` stamps, none doubled at the seam. A varying spacing walks the contour and stops before `len - spacing / 2`.
- `count:n` places exactly `n` stamps per contour, at `(j + .5) * len / n`. It is evaluated once.
- A path with several contours gets stamps on each, in path order. `i` counts over the set across them. A zero-length contour places nothing. A node whose every contour has zero length, such as a dot, with `stamp:` on its own line is E0368. Inherited, it does nothing.
- Cost follows the stamp count: each stamp evaluates the block once.

## stamp: on a node

- `stamp:` is a paint key. It takes a stamp, a list of stamps, or `none`. Like `fill:`, it is valid on every built-in, `copy` included, passes from `layer`, `group`, `inside`, `outside`, `mirror`, `repeat`, `grid`, and `style` to every shape in the block, and a shape's own key wins. A def instance line takes `stamp:` only when the def declares that parameter; otherwise it is E0321. On a boolean operand it is E0321, like `fill:`.
- A bare word that is not a stamp is E0367, and the fix names the nearest stamp in scope. A number or any other kind is E0367. `stamp:[]` is E0367; write `stamp:none` to clear an inherited stamp.
- A stamped open path with no `stroke:` draws no default line: the stamps are its ink. `stroke:` keeps a line, brushed or not, so `brush:bristle stamp:leaves` gives a brushed stem with leaves on it.
- A channel key (`pressure: angle: tilt:`) on a node's own line needs a reader: a brushed stroke, or a stamp whose block or `spacing:` mentions that channel's name. Without one it is E0362 at the key. An inherited channel key that nothing reads does nothing.
- A stamp that places itself, directly or through a def, is E0369 at the `stamp:` key in the block.

## Calling a stamp

- `NAME(k:v...)` returns a new stamp with those keys replaced; the declaration is unchanged. A key that is a declared parameter must match its kind. `spacing:` replaces the placement with that expression, read with the call site's bindings and the bound names on top, so `dots(spacing:gap)` reads the caller's `gap`. `count:` replaces it with a count. `name:` names the set. A positional argument, an unknown key, a kind mismatch, both `spacing:` and `count:` in one call, or a `count:` that is not a positive whole number is E0366, and the notes list the parameters and the placement keys.
- A parameter whose default is a stamp is stamp-typed, like a brush parameter: `def vine stamp:dots` with `curve 0,0 100,0 stamp:stamp(spacing:10)` in the body. An instance passes a stamp or `none`, and a call on the parameter is checked against the default's declaration whatever the instance passed. When the instance passed `none`, the call is `none`.

## Sets, paths and references

- Each stamp of a set is a node under its host with the path `<host>.<set>.<i>`, so the seventh leaf on `vine` is `vine.leaves.6` and its blade `vine.leaves.6.leaf.blade`. The set's name is `name:` on the call, else the stamp's declared name. Two sets with one name on one node is E0333 at `stamp:`; the fix is `leaves(name:small)`.
- A reference reaches a stamp through its set: `@vine.leaves.6` is the stamp, `@vine.leaves.6.leaf.tip` an anchor inside it, and `@vine.leaves.6.cx` a measure. `at:` takes an anchor or a pair, not a node, so place a berry with `at:@vine.leaves.6.leaf.tip` or `at:@vine.leaves.6.cx,@vine.leaves.6.cy`. A patch cannot reach a stamp: `.vine.leaves.6 off` hops through a shape, which is E0340. Change the call or the def instead.
- `explain` lists a stamped node with `stamps:N`, folds a run of three or more stamps of one set into one line with the set's bounds, and prints the first and last stamp in full.

## What a stamp is part of

- Stamps draw after the host's fill and stroke and before its clipped children, in the host's frame, and they are not clipped to the host's geometry: a leaf spills past its stem. They are the host's ink, so the host's `fx`, mask, `a:`, and `blend:` cover them, and `fx outline` traces them. They are not the host's geometry: children, `inside`, `copy`, booleans, and anchors see the spine as before. The host's bounds and bake tiles include its stamps, each stamp's own `fx`, and any brush reach inside it.
- A `blend:` inside the block blends within that one stamp, so a `blend:multiply` leaf darkens only itself, not the stem or the other leaves. A block with no blend draws straight onto the host.
- A stamped node stays vector in `.svg` unless the block is brushed. Then only that shape is baked to a PNG tile, as any brushed node is, and `profile:vector` reports it.

## Seeds and determinism

- Each stamp's seeds come from its own path, so a named host keeps its leaves when other nodes change, and the `positional-seed` lint covers an unnamed one. A stamp whose block names parts, such as `leaves`, needs a stable host: on an unnamed host it is one E0312 at the host's line, and the host is not drawn. `seed:` on the host pins every stamp on it, each with its own numbers, and the result is the same from any host in any file.

## std/stamps

`use std/stamps { dots ticks leaves }` imports the bundled library. `artscript std stamps` prints the file, so `artscript std stamps > lib/stamps.art` forks it. Each takes `tint:` and reads the host's paint when it is `none`.

- `dots r:2 spacing:6 tint:none`: a dot of radius `r` at each placement.
- `ticks reach:4 spacing:8 tint:none`: a 1-unit line across the path, `reach` units to each side.
- `leaves spacing:8+8*pressure tint:none`: a leaf on alternate sides that leans away from the stem by 15 to 40 degrees and grows and spreads with the pressure.
