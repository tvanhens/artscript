# values: the values table, expressions, operators, functions

| kind | examples |
|---|---|
| number | `12`, `.5`, `-3` (canvas units) |
| percent | `50%` of the current frame (the canvas, a parent shape's bounds, a moved group's box around its origin, or a def's `size` box) |
| angle | `30deg`, `.25turn`, bare numbers in `rot:` are degrees |
| time | `200ms`, `2s`, a bare number in a time position is seconds |
| vector / size | `14,0` (no spaces around the comma), `256x256`. A vector is a size, so `rect w,h` sizes from variables. `rect w x h` is E0321 |
| color | `#ff5e5b`, `oklch(.78 .07 55)`, `p.skin.3`, `p.skin` (a ramp's base). A hex color of 3, 4, 6 or 8 digits ends at the first character that cannot continue a name, so `#ff0000+v` is an expression; any other `#` starts a comment |
| list / range | `[a b c]`, `0..8`, `[each k in 0..n -> expr]` |
| brush | `dry`, `dry(strength:.9 dir:45)`, `[chip dry]`; a `brush` declaration, a call on one with overrides, or a stack. See `brushes` |
| reference | `@body`, `@thorax.sheen`, `@hero.head.eye_l`, `@body.neck` (anchor), `@self`; also `canvas`. A path names every shape, instance, and slot on the way down, and nothing else. A group or a layer is never a segment, so a part inside `group face` under `hero` is `@hero.eye_l`, and a path through one is E0350 with a fix that drops it. A bare `@sheen` reaches `sheen` only at the root of its scope. For a `sheen` nested under `thorax` it is E0350, and the fix names `@thorax.sheen`; when `sheen` sits under several shapes, the error lists every path |

A list is indexed with `.0` or with `.{expr}`, where the braces hold any expression that gives a number or a name: `tones.{mod(row 3)}`, `pts.{i+1}`. A name in the braces is read as its value, and a name that is not bound is read as itself, so `p.{skin}` is `p.skin`. A value that is neither a number nor a name is E0320. On a reference, `@dot.{k}` picks one copy of a family the way `@dot.1` does.

Expressions: `+ - * /` (elementwise on vectors; a number scales a vector, as in `(dx,dy)*.28`, `2*v`, `v/2`; `-v` negates), `== != < <= > >= ~=`, `and or not`, `in lo..hi` (`lo` included, `hi` excluded), `if c then a else b` (each branch takes a tuple, so `if c then 3,4 else 5,6` picks a point; write `(if c then a else b),3` to put the `if` inside a tuple). Write binary operators with spaces on both sides or neither: `w/2`, `a + b`. `-x` after a space starts a new argument. Functions: `min max abs clamp lerp sqrt floor round sin cos atan2 mod` (degrees; `atan2(y x)`, floored `mod(a b)`), `polar(r deg)`, `rand(lo hi)`, `pick(list)`, `concat(a b ...)` (two or more lists or ranges joined in order into one list; any other argument is E0320, and `[v]` wraps one item), and the color helpers below. `v.x`, `v.y` read a vector, and `v.0`, `v.1` read the same two numbers, so a table row reads by position at any length. `list.1` reads an item of a list, counting from 0, and `list.{i}` reads the item at the value of the name `i`. `.{name}` works on any member path, so `p.{tone}` reads the palette row that the word in `tone` names. A vector literal inside arithmetic needs parentheses: `a,b + v` is the list of `a` and `b + v`, and `(a,b) + v` adds `v` to the vector. A key that wants a vector and gets that pair reports E0320, and the fix says to parenthesize the vector.
