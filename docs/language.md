# ramose language card

This card and the topic files in `docs/topics/` are the author card for the draft the code implements today. The name of the draft is v0.1. The design is still open. A better design can replace any rule here, and it can break today's outputs.

ramose is a declarative language for procedural art. `ramose build` turns every entry file into PNG, SVG, and GIF. One statement per line: `verb positional… key:value…`, children indented two spaces. `#` starts a comment. `ramose docs <topic> [topic...]` prints the full rules for each topic in the index that `ramose docs` prints after this card.

A line that does not draw says so with its first word: `use palette style finish def rule fn let param brush stamp anim` declare, `if else each` branch and loop, `assert expect` check, `slot` exposes a slot, `play` starts an anim, `ramp color` are palette rows, `fx` adds an effect, `mask` masks, `.part` patches, and `name:` at the end of a line fills a slot. Every other line is a draw: a shape, a structure verb, or a component. `fx`, `mask`, `slot`, `finish`, `anim`, and `play` are reserved, so `let fx = 1` is an error. `light` is reserved for a later draft, and a `light` line is an error today. `brush`, `stamp`, `fn`, and `param` open a declaration only as a line's first word; elsewhere they are plain names, so `brush:` and `stamp:` are keys and `brush` and `stamp` are parameters.

An entry file declares controls with `param view:one_of(normal geometry) guides:false`. Defaults and types follow component parameters. `check`, `build`, and `preview` accept repeated `--set name=value`. `ramose preview art/hero.ramo --target hero.png --crop head --pad 12 --zoom 4 --bg checker --out review/head.png` inspects rendered pixels without changing the canvas or production outputs. See `values` for parameter scope and `cli` for preview controls.

Hold these before you draw:

- The canvas origin is top-left, with y down, so `at:256,256` is the center of a 512x512 canvas. Shapes are centered on their local origin, and `at:` places that origin in the parent.
- **Nesting under a shape clips the children to that shape.** Highlights, shadows, and textures go inside the form they belong to. A boolean is the exception: every child of `union`, `subtract`, `intersect`, or `xor` is an operand, so draw its shading in `inside @<boolean>`.
- A child is placed in its parent's local frame. Under `circle 40 at:100,80`, a child at `0,0` sits at the circle's center. A `curve` or `path` is not centered, so under one a child at `0,0` sits at the `0,0` of its points.
- `warp:corners(tl tr br bl)` pins a node's content box onto four points written in the parent's frame, and `warp:cylinder(r)` wraps it around a cylinder through the node's origin. A list is a stack, applied left to right. The order of application is the content in its own frame, then the warp stages, then `scale:` and `skew:` about `pivot:`, then `rot:`, then `at:`, then the parent; a pinned quad lands where it is written whatever the node's own keys. Everything the node draws follows the warp, effects run after it, and every measurement of the node (`.w`, `.cx`, `.outer`, checks) reports the warped result, never the unwarped content. A warped node is raster-only. See `warp`.
- A brush shades a fill, a stroke, or an outline's ring with SkSL: `brush:dry` on a shape, on a layer for everything in it, or on an `fx outline` line. A brushed stroke is a ribbon that reads `pressure: angle: tilt:` along it. `use std/brushes { dry chip bristle form }` imports the bundled ones. See `brushes`.
- A stamp lays a block of drawing along a path: `stamp:leaves` on a curve draws the `leaves` block once per placement, in a frame that follows the path and reads `pressure` there. The stamps are the node's ink, not its geometry, and they are not clipped to it. `use std/stamps { dots ticks leaves }` imports the bundled ones. See `stamps`.
- Paint with palette ramps, not raw hex. `p` is the manifest's default palette, and `p.skin.3` picks a ramp stop. `palette:elite` on a `@target` or an instance swaps it.
- Name the nodes that matter with `name:`. A named node is addressable from checks, patches, and anims, and it keeps its seed when other nodes change. A name is unique among the parts under the same named shape, or at the root of a file, a def, or slot content, so `name:sheen` may sit under `thorax` and again under `abdomen`. A reference or a patch spells every shape on the way down (`@thorax.sheen`, `.thorax.sheen off`), and never a group or a layer. A name may be a reserved word such as `mask`, in `name:mask` or `layer mask`, because it is only read as `@mask` or `.mask`.
- **A scene's structure does not change over time.** `t` and `key(...)` may appear only in a patch value inside an `anim`. Every node exists for the whole clip, and an anim changes only the values of those nodes. No `if`, `each`, `repeat` or `grid` count, component or slot choice, `path` string, or seed may depend on time, through a `let`, a `fn`, or a def parameter included. An engine animation track needs a node that exists for the whole clip, and that is why the rule exists. `check` does not report a break yet. See `anim`.
- Builds are deterministic on one machine. The same source, manifest, and backend produce byte-identical outputs. GPU output can differ from CPU output and between machines, so a team that compares outputs across machines pins `backend = "cpu"` in the manifest.

A complete entry file:

```
@target plum.png 256x256 scale:1,2

palette orchard
  ramp plum #8e4a8f 5
  color ink #2a1a2e

layer fruit
  circle 80 fill:orchard.plum stroke:orchard.ink,4 name:plum
    ellipse 30,18 at:-28,-34 fill:orchard.plum.4 a:.7 blur:8   # highlight, clipped to the plum
    fx outline 4
```
