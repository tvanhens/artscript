# warp: corners and cylinder, stacks, order of application, the warped box, raster-only

`warp:` maps a node's content through a map that is not affine. The content stays flat and editable in its own frame, and the warp places it, the way an envelope or a corner pin places a smart object. The key goes on any node, next to `rot:`, `scale:`, and `skew:`. Its value is a warp, written as a call the way a brush is, so a `let`, a def parameter default, a patch (`.label warp:none`), and an anim can hold or change it. `warp:none` clears it.

```
rect 40x200 fill:p.cloth warp:corners(412,180 530,196 526,330 408,322) name:side
  rect 40x3 at:0,-70 fill:p.cloth.1

group at:200,150 warp:cylinder(60) name:label
  rect 120x50 fill:p.paper
  rect 80x8 fill:p.ink

rect 60x220 fill:p.cloth warp:[cylinder(40) corners(300,100 360,108 358,330 298,322)] name:spine
let bend = cylinder(60 axis:x)
```

## The two kinds

- `corners(tl tr br bl)` maps the node's content box onto four points, clockwise from the top-left, the order `rect r:` lists its corners. The content box is the node's own ink box in its frame: the geometry plus half the stroke width, plus its stamps, and for a group the box of its children, before any clip or mask cuts it. So a stroked rect lands the outer edge of its stroke on the points. The points are in the parent's frame, so the quad lands where it is written whatever the node's own `at:`, `rot:`, `scale:`, and `skew:`, and a parent that moves carries the quad with it. A stage after `corners`, as in `[corners(...) cylinder(40)]`, still bends around the node's origin, which `at:` places. It is a projective map, so straight lines stay straight. A quad whose edges cross, one with a corner folded inward, or one with no area or three corners on one line, is E0320. A quad written counter-clockwise flips the content. `corners` takes exactly four vectors and no keys; anything else is E0370.
- `cylinder(r axis:y)` wraps the content around a cylinder of radius `r` whose axis runs through the node's origin, viewed straight on, so the content compresses toward the sides that turn away and keeps its size along the axis. `axis:` is `y` (a vertical axis, the default) or `x`. A point `u` from the axis lands at `r * sin(u / r)`. Content farther than `pi * r / 2` from the axis is on the far side of the cylinder and is not drawn, and the `warp-cut` lint (L0108) warns at the node and names a radius that fits. `r` must be a positive finite number, or it is E0320; another count of arguments, another key, or another axis word is E0370.

A `warp:` value that is not a warp, a list of warps, or `none`, such as a number or an unbound word, is E0371 at the key, and so is an empty list.

## Stacks

`warp:[cylinder(40) corners(...)]` is a stack, applied left to right the way `brush:[chip dry]` stacks. Each stage maps the box of the result of the stage before it: the cylinder wraps the content, then `corners` pins the wrapped label's box onto the quad. `[corners(...) cylinder(40)]` pins first and then wraps the pinned quad, so its straight slanted edges bow into curves. A list may hold a `let` that is a warp.

## Order of application

The content in its own frame, then the warp stages in order, then `scale:` and `skew:` about `pivot:`, then `rot:`, then `at:`, then the parent. A `corners` stage is the exception that pins: its points are in the parent's frame, so the node's own keys do not move its quad. Everything the node draws follows the warp: its paint, children, clips, masks, and stamps. Brushes and fields are sampled in the unwarped local frame, so a cloth weave or a noise compresses with its surface. Children are placed in the unwarped frame, and a percent such as `at:50%,50%` reads the flat frame, so the content is laid out flat and then warped. Effects (`fx outline`, `fx glow`, `fx shadow`, and the rest) apply after the warp, so they follow the warped silhouette: an outline hugs the slanted quad, not the flat rect.

A warped node is isolated like a `layer`: a `blend:` on one of its children composites only with what the node itself drew, never with the pixels below it.

## Measurements

Measurements report the warped result, never the unwarped content. `@side.w`, `@side.cx`, `@side.outer`, and every check read the box of the warped node: for `corners` the box of the four mapped corners of the content box, and for `cylinder` the exact box of the wrapped content, since a cylinder compresses along one axis only. `inside @side`, `copy @side`, and `edge @side` read the warped silhouette. A part inside a warped node measures warped too when it is read from outside the node, after the node's body has closed, and so does an anchor under it: `at:@label.title.cx` and the rig anchors in the manifest land where the part is drawn. An anchor that a warp puts out of sight, past a cylinder's visible half or behind a `corners` horizon, has no canvas point: reading it with `@` is E0320, and the manifest leaves it out, with a `warp-cut` warning (L0108) at its `anchor` line. Inside the body of a warped node, the parts read each other, and positions outside the node, in the node's flat frame, because a group's content box is not known until its body closes.

## Anims

An anim patch can set a warp (`.label warp:cylinder(80)`) or clear it (`.label warp:none`). `key(...)` between two warps interpolates the corner points and the radius when both warps have the same stages in the same order with the same axes. Keying between warps of different shapes, or between a warp and `none`, is E0320 at the key.

## Targets

Both kinds are raster-only. A `.svg` target with `profile:svg` bakes a warped node to a PNG tile whose box is the warped box, and `profile:vector` reports E0209 with the feature `warp corners` or `warp cylinder`. Affine transforms stay vector as before. `warp:` on a boolean operand is E0321, like `fill:`; warp the boolean itself.
