# structure: layer and group, clipping by nesting, inside/outside, mask, fx, finish

- `layer name` is an isolated group, and so is each instance of a `def`. A `blend:` inside it composites only with what that layer or instance drew, so a def draws the same pixels wherever it is called. A `blend:` on the call composites the finished instance onto the caller. `group` is a plain group, and its children blend with what is below it.
- **Nesting under a shape clips the children to that shape.** The clip is the shape's geometry whatever its fill, so a `fill:none` shape is a plain clip frame and a translucent shape keeps opaque details opaque. This is how every highlight, shadow and texture stays inside its form:

```
curve 0,-60 50,-10 30,50 -30,50 -50,-10 closed fill:p.peach name:body
  ellipse 30,20 at:-20,-30 fill:p.peach.4 a:.6 blur:12   # soft light, upper left
  subtract fill:p.peach.1 a:.5 blur:2                      # cel shadow, lower right
    copy @body
    copy @body at:-14,-10
  fx rim 4 dir:1,1
  fx grain .04
  fx outline 4
```

- `inside @torso` draws its block clipped to another node's silhouette, and `outside @x` does the inverse.
- A mask is a child line of the draw it masks, written `mask @body`, `mask simplex(30) > .5`, or `mask` alone over an indented block. It takes a value or a block, not both, and a draw has at most one. It may sit anywhere among the children. Add `feather:6` on the draw for a soft edge. `mask:` with a colon is a slot fill for a slot named `mask`, not a mask.
- `fx` lines apply to the draw or `def` they sit directly under, in order. They cannot go at the top level, in a palette, or directly under `if` or `each`. A draw inside an `if` still takes its own `fx`. Use `blur 4`, `shadow 0,6 blur:8`, `inner 0,4 blur:4` (a shadow cast inward from the top edge), `glow 8`, `rim 3 dir:1,1` (a light band on that side), `adjust hue:10 sat:-.2 light:.05`, `outline 4` (a line around the shape geometry behind the node, default `line(fill)`, unchanged by an earlier blur, shadow, or glow, at its color's own alpha, and never visible through a translucent fill), and `grain .05 scale:1.5`. Each takes `color:` and `a:`. `as:name` names a result and `of:name|@node|self` picks an input, and `merge a b a:.4,1` combines them.
- `fx outline` traces the filled areas of the node and its subtree, and a stroke adds its own filled path. Children add their areas, clipped to the parent, only under a parent whose fill shows nothing. Every area is filled and stroked at twice the width with round caps and joins, and the union is drawn once at the outline color's own alpha, so overlaps do not darken it. The outline is erased inside every translucent area: a fill or stroke with any stop below alpha 1, or a descendant with `a:` below 1. A glass, ice, or ghost body keeps a full-strength ring and shows no ink through itself. The outlined node's own `a:` fades the whole node, outline included.
- `finish` names an ordered stack of `fx` lines that several nodes share:

```
finish surface width:4 a:.45 strength:.04 scale:2.5
  fx rim width dir:1,1 color:p.rim a:a
  fx grain strength scale:scale

curve … name:cap
  fx surface width:5 a:.55
  fx outline 7
```

`fx surface` puts the finish's lines into that node's effects at that line, so the outline above still comes last. Only that node gets them, never its children. Parameters are named and typed by their defaults, and the arguments are evaluated where `fx surface` is written. Other names in the body resolve in the file that declares the finish. Grain still seeds from the node it lands on, so a finish draws the same as writing its lines out. A finish is imported like a style (`use fx/surface { surface }`). Its body holds only built-in effects, with no `of:`, no `as:`, and no other finish. `finish:surface` as a node key is an error; write `fx surface` under the node.
