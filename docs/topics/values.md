# values: the values table, expressions, operators, functions

| kind | examples |
|---|---|
| number | `12`, `.5`, `-3` (canvas units) |
| percent | `50%` of the current frame (the canvas, a parent shape's bounds, or a def's `size` box) |
| angle | `30deg`, `.25turn`, bare numbers in `rot:` are degrees |
| time | `200ms`, `2s`, a bare number in a time position is seconds |
| vector / size | `14,0` (no spaces around the comma), `256x256`. A vector is a size, so `rect w,h` sizes from variables. `rect w x h` is E0321 |
| color | `#ff5e5b`, `oklch(.78 .07 55)`, `p.skin.3`, `p.skin` (a ramp's base). A hex color of 3, 4, 6 or 8 digits ends at the first character that cannot continue a name, so `#ff0000+v` is an expression; any other `#` starts a comment |
| list / range | `[a b c]`, `0..8`, `[each k in 0..n -> expr]` |
| reference | `@body`, `@hero.head.eye_l`, `@body.neck` (anchor), `@self`; also `canvas`. After an instance, a name finds the part under any group in it, so write `@hero.eye_l`, not `@hero.face.eye_l` for a `group face`. A hop through a group, a layer, or a shape to a part that is not an anchor is E0350, and the fix names the part's key |

Expressions: `+ - * /` (elementwise on vectors; a number scales a vector, as in `(dx,dy)*.28`, `2*v`, `v/2`; `-v` negates), `== != < <= > >= ~=`, `and or not`, `in lo..hi` (`lo` included, `hi` excluded), `if c then a else b` (each branch takes a tuple, so `if c then 3,4 else 5,6` picks a point; write `(if c then a else b),3` to put the `if` inside a tuple). Write binary operators with spaces on both sides or neither: `w/2`, `a + b`. `-x` after a space starts a new argument. Functions: `min max abs clamp lerp sqrt floor round sin cos atan2 mod` (degrees; `atan2(y x)`, floored `mod(a b)`), `polar(r deg)`, `rand(lo hi)`, `pick(list)`, and the color helpers below. `v.x`, `v.y` read a vector.
