# checks: assert, expect, measurements, check functions, rule

Checks measure the evaluated scene and never move anything. An `assert` failure is an error, and an `expect` failure is a warning.

```
assert level(@eye_l @eye_r) tol:.5
assert inside(@hero canvas margin:24)
expect @head.w / @body.w in .9..1.4 "head as wide as the body"
```

Measurements are `@a.x .y .w .h .cx .cy .left .right .top .bottom .area`. They measure the geometry plus half the stroke width, or an unbrushed tapered stroke's outline, and a shape's stamps (see `stamps`), and they ignore effects. `@a.geom.w` and the other `.geom` measurements read the geometry alone, with no stroke and no stamps. A group is measured by its content cut to its `inside` clip and its mask. An `outside` clip and a field mask keep the whole content. Functions are `inside(a b margin:)`, `overlaps`, `touches`, `gap`, `dist`, `aligned(a b edge:top)`, `level`, `centered(a in:b)`, `symmetric(a b about:x)`, `aspect`, `on_grid(a n)`, `same_size(@row.*)` and `stroke_widths(a)`. `@row.*` lists the named children of every copy. Inside a loop, `@dot` is that copy, including from a slot in the loop and from a second loop beside the one that draws it. A check inside a def can use `@self` when the call has no `name:`. A draw that reads `@self` is an error. `rule name subject:@self` + checks bundles them and is applied with `assert name(@x)`.

`@a.outer` is the box of everything `@a` draws: its content, with each stroke measured by the outline it fills so miter joins and square caps count, plus the reach of its `fx outline`, `glow`, `shadow`, and `blur`, and of a shape's `blur:` (see `structure`). `@a.outer.bottom` reads one measurement of that box, and `assert inside(@a.outer canvas)` fails when an effect is cut by the canvas edge. `.outer` is a box, so `copy @a.outer` or `inside @a.outer` uses that rectangle, not the silhouette. Plain `@a` measurements and `inside(@a canvas)` still measure the geometry plus half the stroke, and a shape's stamps. The node's own clip and mask cut its content and not its effects, because effects draw after them. An effect that reads another node with `of:@b` reaches where `@b` draws. A part named `outer` wins over the box, as a part named `w` wins over `.w`.

The `off-canvas` lint (L0102) warns when a node's drawn extent, the same box as `.outer`, passes the canvas edge by more than 1 unit. Each shape is measured, except a shape nested under another shape, which that shape clips. An `inside` clip or a node mask on a group cuts what its children draw. A group or layer with effects is measured too, and it is reported at its own line only when none of its shapes is, so one cut is reported once. A `viewport` target skips this lint.

The `hidden-child` lint (L0105) warns when a line nested under a shape draws nothing, because each node it draws there lies wholly outside that shape or a clipping shape above it. Each node is measured by its drawn extent, the same box as `.outer`, against the clipping shape's geometry box. A loop is reported only when every copy is hidden, so a scatter that the shape trims is not reported. An animation reports the line only when it is hidden on every frame, so a blink that covers a highlight is not reported. A `viewport` target still runs this lint.
