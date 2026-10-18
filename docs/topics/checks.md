# checks: assert, expect, measurements, check functions, rule

Checks measure the evaluated scene and never move anything. An `assert` failure is an error, and an `expect` failure is a warning.

```
assert level(@eye_l @eye_r) tol:.5
assert inside(@hero canvas margin:24)
expect @head.w / @body.w in .9..1.4 "head as wide as the body"
```

Measurements are `@a.x .y .w .h .cx .cy .left .right .top .bottom .area`. A group is measured by its content cut to its `inside` clip and its mask. An `outside` clip and a field mask keep the whole content. Functions are `inside(a b margin:)`, `overlaps`, `touches`, `gap`, `dist`, `aligned(a b edge:top)`, `level`, `centered(a in:b)`, `symmetric(a b about:x)`, `aspect`, `on_grid(a n)`, `same_size(@row.*)` and `stroke_widths(a)`. `@row.*` lists the named children of every copy. Inside a loop, `@dot` is that copy, including from a slot in the loop and from a second loop beside the one that draws it. A check inside a def can use `@self` when the call has no `name:`. A draw that reads `@self` is an error. `rule name subject:@self` + checks bundles them and is applied with `assert name(@x)`.
