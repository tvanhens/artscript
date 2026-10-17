# artscript semantics

These notes describe the draft the code implements today. The name of the draft is v0.1. The design is still open. A better design can replace any rule here, and it can break today's outputs. When you change a rule, update this file, `docs/language.md`, and the matching file in `docs/topics/` in the same change.

`docs/language.md` is the author-facing core card for the same draft, and `docs/topics/` holds the author-facing rules for each topic. This file records details that the card and the topics leave out. When they disagree, choose the better behavior and make them agree.

## 1. Lexing

- **Lines.** A statement is one line. The only multi-line token is a `"""` raw string. Blank lines and comment-only lines are skipped and do not affect indentation.
- **Indentation.** Indentation is spaces only, and a tab in indentation is error E0102. A line indented deeper than the previous statement opens a block under it. A dedent must return to an enclosing level, otherwise it is E0103 and the note names the nearest valid parent line.
- **Comments vs colors.**
  - `#` followed by exactly 3, 4, 6 or 8 hex digits and then a delimiter (whitespace, end of line, `,`, `)`, `]`) is a color.
  - Any other `#` starts a comment that runs to end of line.
- **Numbers.**
  - Numbers look like `12`, `1.5`, `.5`. There is no exponent, and the sign is never part of the token.
  - The suffixes `%`, `deg`, `turn`, `ms`, and `s` attach with no space. A percent is stored as 0.5 for `50%`, and a turn as 360 degrees. `ms` and `s` store seconds. The lexer tries `ms` before `s`.
- **Sizes.** A number or percent, then `x`, then a number or percent with no spaces is one size token: `256x256`, `100%x60%`, `8x40`. `mirror x` stays an identifier because `x` is not glued to a number.
- **Identifiers.** Identifiers match `[A-Za-z_][A-Za-z0-9_]*`. Every grammar word has one row in the word table in `crates/syntax/src/words.rs`. Keywords are `use as from palette style finish def rule let shader light brush assert expect if then else each in and or not true false none fx mask slot anim play`. The future words (`sksl field warp backdrop profile pose joint ease t frames key clip loop set mark root visible`) are reserved too. A reserved word used as a name is E0105, so `let fx = 1` and `let t = 1` are errors. `ramp` and `color` head palette rows and are ordinary names everywhere else, so `color:p.ink`, a def parameter named `color`, and the `ramp(...)` builtin keep working. `in` is also a value word, so `ease:in` parses. A reserved word in expression position is a value, so `let n = t`, `key(...)`, and `name:mark` parse. The same word as a binding, a parameter, or a draw verb is still E0105.
- **References.** `@` followed by an identifier, then `.segment` repeats where a segment is an identifier, digits or `*`.
- **Member access.** A `.` glued to the end of an identifier, a `)` or a reference continues a member chain (`p.sky.3`, `v.x`). A `.` at the start of a statement followed by a name is a patch. A segment may be a keyword, because a slot may be named one: `slot mask`, then `.mask off`.
- **Strings.** `"..."` supports the escapes `\" \\ \n`. `"""..."""` is raw and may span lines. Its common leading indentation is stripped.
- **Whitespace.** Every token records whether whitespace preceded it, and the parser needs this.

## 2. Parsing

The first word of a line decides its form, so a line that does not draw says so up front:

| line starts with | form | where it may go |
|---|---|---|
| `@ident` before any statement | directive. `@target` takes a raw path token (everything up to whitespace), then args, named args, and an optional `each`. A known target key (`bg`, `scale`, `as`, `fps`, `dur`, `t`, `clip`, `profile`) is never an axis, even when the value is a list. Any other named item after `each` is an axis. `fmt` prints `clip:` after `each` | file top |
| `use` | `use <raw path> [as x] [{ a b }]` | file top |
| `palette`, `style`, `finish`, `def`, `rule`, `shader`, `light`, `brush`, `anim` | declarations. In `finish`, `def` and `rule` every parameter is a named arg. `def` accepts `needs:@a` or `needs:@a,@b` as a parameter named `needs`. A `style` takes no block, and one is E0101 whose fix names `finish`. A `finish` needs a block, and every line in it must be an `fx` line, or it is E0101. `anim <name> dur:<time> [loop]` needs a block of patch lines (E0341 otherwise). A path segment may be `*` | file top |
| `ramp`, `color` | palette row: `ramp <name> <color> [<color>] [<count>] [hue:h]` or `color <name> <color>` | directly under `palette`, and nothing else may go there |
| `let` | `let name = expr` | any scene block |
| `assert`, `expect` | `expr named* "message"?` | any scene block |
| `if`, `each`, `else` | control. `else` must directly follow an `if` block at the same indent. `else if` chains are allowed | any scene block |
| `slot` | `slot <name> named*` with an optional fallback block. The name may be `mask` | any scene block. Evaluation requires a `def` (E0334) |
| `play` | `play <name>\|none [start:<time>]` | entry file top, and under an instance (E0342 otherwise) |
| `fx` | `fx <name> items`, with no block. The name is a built-in effect or a finish | directly under a draw, a `def`, or a `finish` |
| `mask` | `mask <expr>`, or `mask` alone over a block. One per draw, anywhere among its children | directly under a draw |
| `.ident` | patch. Path segments, named args, and an optional trailing `off`. A segment may be `*` | directly under a draw, and under an `anim` |
| `ident:` then end of line | slot fill, which needs a block. `mask:` is a slot fill named `mask`, not a mask | directly under a draw |
| any other identifier | draw: the verb (a shape, a structure verb, or a component), then items | any scene block |

A scene block is the file top, a draw's children, a `def` body, and the bodies of `if`, `else`, `each`, `slot`, a slot fill, a mask block, and a `rule`. A line in a block that cannot hold it is E0101, and the line and everything indented under it are dropped. A `def` body holds draws, control, checks, `slot`, and `fx` (effects attach to the instance). A mask, a slot fill, or a patch directly in a `def` body is E0101. A draw nested inside the def still takes its own mask.

**Colons at the head.** An identifier (or `mask`) glued to `:` at the end of a line is a slot fill. Glued to `:` with more tokens after it, as in `mask:simplex(30) > .5` or `fill:#fff`, it is E0101 with the fix "write `mask simplex(30) > .5`, or end the line with `mask:` to fill a slot". Named args in the item list (`hue:12`, `as:soft`, `color:p.ink`) are unaffected. A `mask:` named arg on a draw is E0101, because the mask is its own child line.

**Masks.** A mask has a value or a block, never both and never neither. A second mask on the same draw is E0101. `fmt` prints a draw's children in source order and never moves a mask, an `fx`, or a child.

**Items on a line.** Items are separated by whitespace. An item is either a named arg or a positional expression.

- A named arg is an identifier glued to `:` with the value glued after it.
- A trailing string literal on a node is a positional `Str`.

**Expressions in item position.**

- A binary operator continues the expression only when it has whitespace on both sides or on neither side. `w / 2` and `w/2` are both one expression. In `a -b`, the `-` is glued to `b` but spaced from `a`, so it starts a new item.
- The keyword operators (`and or in -> ..` and the comparisons) continue the expression when they are spaced.
- A tight comma, with no whitespace on either side, builds a `Tuple` at the lowest precedence of that tight run: `-w/2,0` is `Tuple[-(w)/2, 0]`, and `p.ink,1.5` is `Tuple[p.ink, 1.5]`.

**Inside `( )`, `[ ]` and `use … { }`.** There is one way to write this.

- Items are separated by whitespace only: `shade(c -2)`, `level(@a @b)`, `[plate leather]`, `use face/parts { eye blush }`.
- A comma with whitespace on either side is error E0101, with the fix "remove the comma".
- A tight comma builds a tuple exactly as it does on a statement line. So `radial(a b at:.3,.3)` is a call with a vector argument, `[5 4 3]` is a list of three numbers, and `[0,0 10,5]` is a list of two points.
- `[each name in <iter> -> <expr>]` is one list, not a list of items. The iterator stops before `->`, and the body is one expression, so `x,y` is a point.
- A statement `if` given `then` is E0101, with the fix "statement `if` takes a block; remove `then`".
- Named args are allowed in calls. `oklch(.78 .07 55)` is an ordinary call.

**Precedence**, lowest to highest:

1. `if-then-else`
2. `or`
3. `and`
4. `not`
5. comparisons and `in`
6. `->`
7. `..`
8. `+ -`
9. `* /`
10. unary `-`
11. postfix (member, call)
12. primary

So `1/8..1/6` parses as a range of two quotients, and `(.8..1.2) * w` needs its parentheses.

**Errors.** Parse errors are E0101 with a concrete fix. Report every error, then recover at the next line.

**Formatting (`fmt`).**

- Indentation is two spaces per level, with a single space between items.
- Positional args come before named args, in source order.
- Numbers drop a leading zero and trailing zeros (`0.50` becomes `.5`, `1.0` becomes `1`).
- The operators `+`, `-`, `*`, and `/` are written with no spaces. `cx-40,884` is a tuple of a subtraction and `884`. `cx - 40,884` and `(cx - 40),884` are both written `cx-40,884`.
- Parentheses are printed when removing them would change the tree. `(a+b)*c` and `a+(b,c)` keep their parentheses.
- Comments are preserved: keep each comment's text attached to the statement that follows it, or trailing on its line. The AST does not hold comments, so `fmt` re-reads them from the source by line.
- One blank line between statements is kept. Several blank lines in a row become one. Blank lines are not in the AST, so `fmt` re-reads them from the source.
- `format(parse(format(x))) == format(x)`, and `parse(format(x))` is the same tree as `parse(x)`.

## 3. Coordinates and frames

- Canvas origin is top-left, with y down.
- Every node has a local frame. `at:` translates its origin in the parent.
- The transform is `T(at) · T(pivot) · R(rot) · Skew(skew) · S(scale) · T(-pivot)`.
- Primitive geometry is centered on the local origin. Curve, path and copy points are taken as written.
- **Frame rect** is the rect that percentages and defaults resolve against:
  - At file top level it is the canvas `(0, 0, W, H)`.
  - Inside a layer or group, it is the parent frame mapped into the group's local coordinates.
  - Inside a shape's children, it is the shape's local geometry bounds.
  - Inside `inside @x`, it is `@x`'s bounds in local coordinates.
  - Inside a `def` body, it is the instance's own box when the def declares `size`, and an empty rect at the origin otherwise. The box is that size centered on the local origin. `size:40` is a 40 by 40 square. `size:72x32` is 72 wide and 32 tall.
  - Inside a slot, it is the enclosing frame's size mapped through the slot's transform, centered on the slot origin, for the caller's fill and for the slot's own block. So content with no `at:` stays on the slot.
- **Percentages.**
  - A position percent is `frame.min + p * frame.size` on that axis.
  - A size percent is `p * frame.size`.
  - A plain number is always a local coordinate.
  - A percent of an empty frame is E0322, not 0. Inside a def, the message names the def.
  - A percent passed as `size` resolves against the caller's frame mapped into the instance. A pair uses each axis. A bare percent uses the smaller side, as `circle` does. The body then sees `size` as those numbers. A non-positive or non-numeric `size` is E0320.
- **Default `at:`.**
  - For a primitive shape, the default is the frame center. So `rect 100%x100%` at top level fills the canvas, and a child circle defaults to its parent's center.
  - For layers, groups and instances, the default is `0,0`.
  - Inside a def, the box center is the origin, so a primitive with no `at:` stays on the instance origin.

## 4. Node identity

- Every node has a key. Segments are joined by `.`. A segment is a name, a copy index written in digits, or a positional segment `verb~N`. A mirrored copy's key ends in `~m`.
- **Names.** A name is an identifier `name:`, the positional identifier of `layer` or `group`, a slot name, or an anchor name. A def name is not a name, so a call `eye` with no `name:` is unnamed. A `name:` that is not one identifier, or a second name on one node (`layer hat name:cap`), is E0312. The fix writes one identifier `name:`.
- **Scopes.** A name lives in a scope. A scope is the file, one def body per instance, or a slot's content. A layer, a group, a shape, a loop, an `if`, a mirror, and a mask do not open a scope, and they do not appear in a named key. The old path hung every node from its tree parent, so wrapping `name:head` in `group face` renamed the head and moved its seed. The key stays `head`.
- **Uniqueness.** A name is unique in its scope at every depth. The check reads the source when the module loads, so it fires even when the statement is not drawn. The second declaration is E0333 at its line. The note names the first line. The fix says to rename one. Two declarations in exclusive arms of one `if` may share a name. `name:lips` in two defs is legal. `name:form` in six slot fills is legal. `name:beam` on a group and on its child in one file is E0333.
- **Named keys.** A named node's key is its scope's key, then its name, then one index for each `repeat`, `grid`, or `each` between the scope root and the statement. The file's key is empty. An instance's body uses the instance's key. A slot's content uses the slot's key. `hero name:h` whose body holds `circle name:eye` is `h.eye`.
- **An unnamed call that holds a name.** An explicit name hangs from the call that opened its scope. When that call's key is not stable, the call is E0312 and the fix is `add name:`. The body is still evaluated. A `name:` that is not one identifier is already E0312, and the call is not reported again. Without this, `name:shaft` inside an unnamed `_pillar` becomes `_pillar~2.shaft` when another `_pillar` is inserted.
- **Unnamed keys.** An unnamed node's key is its tree parent's key, then `verb~N`, then the loop indices since that parent. `verb` is the word written, so an alias counts under that word and not under the def name. `N` is 1-based and counts every draw with that verb in the block, named draws included. Both arms of an `if` count. An `each` body counts once. The count does not enter a child block, and it evaluates nothing. `verb~N` is not an address. The old suffix omitted index 1 and wrote `circle2`, which collided with a name an author could write.
- **Copies.** `repeat`, `grid`, and `each` add one index, from 0. Grid order is `row * cols + col`. Inside the loop, `@eye` is the current copy. A second loop beside the one that draws it keeps that outer copy, and an index then counts the loops still open, so `@dot.0` in the outer loop is the first inner copy. That holds when the inner loop drew one copy. Outside it, a consumer that needs one node and gets the family is E0343. The note lists `.eye.0` and `.eye.1`. `same_size` and a final `*` accept the family. A final `*` on a family lists the named children of every copy, so `@row.*` is those children. A later hop stays on the copy it named. `@row.1.gem.0` is copy 0 of `gem` under copy 1 of `row`, and `@row.0.*` does not include copy 1. An index that is not a whole number is not an address. The old `#i` suffix sat only on the copy's top node, and `#` starts a comment, so a patch could not spell a copy. Requiring one parent before `*` used to reject `@row.*` with E0343.
- **Mirror.** `mirror` evaluates its body once. Patches apply to the original. The clone's keys gain `~m` and the clone draws no seeds. `@ear` is the original. `.ear~m` is not a patch target. Both halves move together. The old `_m` suffix was a second spelling, and a patch `.ear_m` did not reach the copy.
- **Stable keys.** A key with no `~` is stable. A stable key is the address, with a leading `.` for a patch and a leading `@` for a reference. `.eye` is the family. `.eye.0` is one copy. `*` is a final hop only, and it matches the named nodes whose tree parent is the node before it. When that node is a family, the match is the children of every copy.
- **Contract.** A node's key changes only when its own name, a name on the scope it hangs from, or a loop index around it changes. A node with no name also changes when a draw with the same written verb is added, removed, or reordered in its block, and when its tree parent's key changes. Wrapping it in `group name:face`, renaming that group, or renumbering an unnamed ancestor is that parent change. An explicit name never hangs from an unnamed segment or from a group, so those edits do not apply to it.

## 5. Values and types

- **Runtime values** are Num, Vec2, Size, Percent, Color (which remembers its ramp and index if it came from one), Ramp, Palette, Paint (gradient), Field, Range, List, Str, Ident (enum-like words such as `slim`, `down`, `closed`), Component, Ref, Bool and None.
- **Range `a..b`.** The range holds every number `x` with `a <= x < b`. `x in a..b` uses that test. `each` visits `a`, then `a + 1`, then `a + 2`, while the value stays below `b`, so `each i in 0..n` runs `n` times and the last `i` is `n - 1`. When `b <= a`, the range is empty. `in` matches nothing and `each` runs zero times. `pick`, one-argument `min` and `max`, `dash:`, `rect r:`, and merge `a:` read those steps. Two ranges compare equal when both are empty, or when both ends match. A range does not equal a list. A range is not a stroke width. `stroke:c,a..b` is E0320, and the fix is `stroke:c,[a b]`.
- **Def parameter types** are inferred from their default. Passing a different kind is E0320, and the fix names the expected kind.
- **Ramp as a color.** A ramp used where a color is expected is its base stop.
- **Identifiers** resolve in this order: `let` and loop bindings, def params, the `target` constant and matrix variables, palettes and aliases, components, and finally bare enum words.
- **Arithmetic.** `+` and `-` on two vectors add and subtract. `*` and `/` on two vectors are elementwise. A number on either side of `*` scales a vector, and `v/n` scales by `1/n`. Unary `-` negates a vector. A pairing that is none of those is E0320.
- **`atan2(y x)`** returns degrees, the same unit as `sin`, `cos`, `polar` and `rot:`. **`mod(a b)`** is the floored remainder (`mod(-1 3)` is 2). A zero divisor is E0320.

## 6. Color

- Implement OKLab and OKLCH conversion as in `../chibi/.cursor/skills/chibi-art/toolkit/src/chibi/color.py`.
- Out-of-gamut colors are mapped by reducing chroma (binary search), never by clipping.
- **Single-color ramp `ramp <name> c n [hue:h]`.** The row takes one to three positionals and no named arg but `hue:`. Evaluation decides which positionals are colors and which is the stop count (default 5), because a name can be either.
  - The base index is `b = (n - 1) / 2` (integer division).
  - The step `s = i - b` produces each stop. With base `L, C, H` and `h = hue` (default 15):
    - For `s < 0`:
      - `L' = L - 0.125*|s|`
      - `C' = C * (1 - 0.1*|s|) + 0.005*|s|`
      - `H'` rotates toward 275 by `min(h*|s|, distance)`
    - For `s > 0`:
      - `L' = L + min(0.075*s, (0.985 - L) * 0.4 * s)`
      - `C' = C * (1 - 0.12*s)`
      - `H'` rotates toward 75 by `min(0.65*h*s, distance)`
  - A neutral base (C < .02) keeps its hue.
  - Out-of-range steps (`shade` past either end) extrapolate with the same formula.
- **Two-color ramp `ramp <name> a b n`.** Interpolate in OKLCH, taking the shorter hue arc.
- **`line(c)`** is `L = max(L - .42, .18)`, `C = max(C * 1.15, .04)`, with the hue rotated 24 degrees toward 275.
- **`shade(c k)`**:
  - If `c` remembers a ramp, use index + k, extrapolating past the ends.
  - Otherwise, apply the step formula with `s = k`.
  - `lit(c k)` is an alias.
- **`mix`** interpolates in OKLab. **`fade(c a)`** multiplies alpha.
- **Gradients.**
  - Densify each segment to 16 sub-stops interpolated in OKLCH, with linear alpha.
  - When one end has alpha 0 or chroma < .02, take hue (and, for alpha 0, the color too) from the other end.
  - Emit sRGB `Stop`s.
  - When the first two arguments of `grad` are points, they are the start and end in the shape's local coordinates. A bare number is an angle, not a point, so `grad(17 a b)` keeps the direction form. Otherwise `grad(dir ...)` maps `down` to bounds top-center through bottom-center, and so on. For an angle, the line passes through the bounds center and spans the bounds' projection on that direction. Stop positions are unchanged.
  - `radial` defaults `at:.5,.5`, and `r` defaults to the distance from the center to the farthest corner.
- **Raw color lint.** Literal colors outside `palette` blocks trigger the `raw-color` lint (L0101, warning by default). `#fff`-class near-whites and `oklch(...)` literals inside def parameter defaults are not exempt.

## 7. Paint and geometry defaults

- **Fill.**
  - Closed shapes with no `fill:` and no inherited style fill get `fill:#8a8a8a`.
  - Open curves have no fill.
  - An open curve with no stroke gets `stroke:line(#8a8a8a),2`, or the style's stroke if one is set.
- **Stroke.**
  - `stroke:c,w` gives a constant width with round cap and join.
  - `stroke:w` alone uses `line(fill)` (or `line(#8a8a8a)`).
  - `cap:` and `join:` override, and `dash:4,2` dashes.
- **Tapered ink.** A stroke whose width is a list, or a stroke with `taper:` set, becomes a filled outline (`Shape.fill = stroke paint`, no stroke). Use the `brush` algorithm from chibi `paths.py`:
  1. Sample the curve every 0.75 output pixels. In the path's own units that step is `0.75 / (transform scale × target scale)`. The transform scale is the accumulated scale (the longer axis of the local-to-canvas matrix). The target scale is the output's own scale for a PNG, so `name.png` has the same bytes with or without `@2x`, and `@2x` stays as smooth as `scale:1`. An SVG uses the target's highest `scale:`. A GIF uses scale 1, including when `scale:` or the manifest lists a higher number. A stroke in unit space under `scale:50` then matches the same stroke written in pixels.
  2. Interpolate the per-point widths by chord fraction.
  3. Taper each end with the floor `.12` and ease `sin(x*pi/2)^.8`.
  4. Add round caps.
  - A single width with `taper:` is that width everywhere.
- **Curve.** Use centripetal Catmull-Rom with tension (default .5), exactly as `_spline` in chibi `paths.py`. `closed` closes the curve. Points are the positional arguments. When the only positional argument is a list, its items are the points (`curve pts`, `curve [each k in 0..n -> x,y]`). A list beside other points is E0320. There is no separate polygon-of-points verb: a closed curve is that polygon.
- **Blob.**
  - Use `points:` (default 7) points on a circle of radius r.
  - Each radius is scaled by `1 + wobble * u` with `u` in -1..1 from the seed, and each angle is jittered by `±wobble * 0.5 * step`.
  - The result is joined with a closed curve.
- **Poly and star.** The first vertex points up. `round:r` rounds corners with Skia's corner path effect, then resolves the result to a path. A closed `curve` at `tension:0` is straight sides, and `round:r` rounds those corners with the same effect. A smooth curve has no corners, so `round:` is read only at tension 0.
- **Booleans.** Use PathOps across the operands' transformed geometries, in the boolean node's local coordinates.
- **`copy @ref`.** Takes the referenced node's silhouette (the union of its shape geometries, transformed into current local coordinates).
- **`jitter:amp,wavelength`.** Apply chibi's `wobble`, seeded from the node key. The pair is the only spelling: a bare number would hide the wavelength, and a second key would split one setting. A bare number is E0320, and the fix writes the amplitude with wavelength 60, which is what older art meant. Wavelength must be positive.
- **`blur:` on a shape** sets `Shape.blur`.

## 8. Effects (`fx`)

- `fx` is a child line of a draw or a `def` and is appended to that node's `effects`, in source order. On a component, the def body's `fx` run first, then the call site's. The line `fx <op> positional… named…` maps onto `EffectOp`. At the file top, in a palette, or directly under `if`, `else`, or `each` it is E0101. A draw nested under `if` still takes its own `fx`.
- `color:` defaults:

| op | default color |
|---|---|
| shadow, inner | `shade(f -2)` at alpha .35 |
| glow | `shade(f 2)` |
| rim | `oklch(.86 .05 265)` |
| outline | `line(f)` |

  Here `f` is the node's fill (the first solid stop of its fill, or the first descendant shape's fill), or `#8a8a8a` when there is none.
- `a:` multiplies the effect color's alpha.
- `rim` takes `dir` (default `1,1`).
- `grain` is a 3-octave Perlin overlay, in the style of chibi `pen.grain`. Octave weights are 1/2, 1/4, and 1/8. `scale` is the finest cell size in canvas units (default 1.5). The seed comes from the node key. Overlay opacity is `min(1, 2 * strength)`, so strength .05 swings a mid gray by 5% at the peaks. Chibi divides the octave sum by that crop's peak. The shader divides by 0.58, the peak of this noise on a flat shape.
- `outline` traces the node's shape geometry and draws it behind the content. Earlier `blur`, `shadow`, and `glow` do not change the traced path. The areas are the filled geometry of the subtree. A shape's children are omitted, because they composite source-atop and do not change its alpha. A stroke contributes that stroke's filled path. Each area is filled and stroked at `2 * width` with round cap and join, in the outline color.
- `as:` sets `Effect.name`. `of:` takes `self`, a name, or `@node`.
- **Finishes.** `finish name params` declares a named list of `fx` lines. It is not a value. It lives in the module's finish table beside `styles`, and `let`, def parameters and `m.name` cannot reach it.
  - Declaring checks every line. A Phase 2 effect is E0391. A line naming anything but a built-in effect, including another finish, is E0338. `of:` or `as:` is E0336. Bad lines are reported and dropped, and the finish is still declared. A finish named after a built-in effect is E0337.
  - `fx name args` with a finish name puts that finish's lines into the node's effects at that position, in order. Lines before and after it keep their order. The node's children never receive them, and `Scope` carries no effects.
  - Arguments must be named. A positional argument or an unknown name is E0321, and the note lists the parameters. Arguments are evaluated at the `fx` line. They are bound like def parameters: a value whose type differs from its default's is E0320, and a missing one takes the default. The body is evaluated in a scope of the declaring module with those parameters bound, so `p.rim` means that module's `p`.
  - `grain` seeds from the file where the `fx` line is written and the key of the node it lands on, exactly as if the line were written there. Moving a stack into a finish leaves the output unchanged.
  - Errors inside a finish are reported at the `fx` line, with a note naming the finish and its declaration.
  - In a def, effects from the def body come before the effects from the call's block.
  - `finish:name` as a node key is E0339, and the fix is "write `fx name` under the node". `style:name` naming a finish is E0307, with the same fix.

## 9. Components, slots and overrides

- **Calling a def.**
  1. Bind its params (positional first, then named).
  2. Collect the instance block. It may contain slot fills, patches, checks, `fx`, and one `mask`. Anything else is E0330, and the fix suggests wrapping it in a slot fill. The instance's mask is the call site's only. A slot fill or patch under a draw that is not a component is E0331 or E0332.
  3. Evaluate the body in a fresh scope whose frame is the instance box. That box is the resolved `size`, centered on the origin. A def with no `size` keeps an empty frame.
  4. The instance is an isolated group, like a `layer`, and `explain` prints it as one. A `blend:` in the body, `erase` included, composites with what the body drew and never reaches the caller's pixels. Slot content is drawn inside the instance, so it is isolated with it. A `blend:` on the call composites the finished instance onto the caller. An instance used to be a plain group. Then `blend:erase` in a def punched through the caller's art, and an opacity, effect, mask, or clip on the call changed what a blend in the body produced.
  5. A def may reuse a shape word. `def circle` is a component, the call is an instance, and a patch can hop through it. A slot fill under that call is its own scope. The static check sees defs and imports in the file. A `let` in an entry file that binds a shape word to a component is not part of that check. Treating every built-in word as a draw used to reject `.c.child` on `def circle`.
- **Patches.** A patch names a declared name. `.eye fill:#ff0000` reaches `name:eye`. The old rule matched a relative path, so a long patch could land on a later node that happened to share that path.
  - Every segment before the last is an instance or a slot, or a copy index. The patch is forwarded into that body or that slot content, one segment at a time.
  - The last segment names a node in the scope it reaches. A bare family name reaches every copy. A copy index after it reaches one. A final `*` reaches the named nodes whose tree parent is the node before it. A final `*` in a scope that declares no name is E0340.
  - A name starting with `_` in a def body, a slot fill, or a slot fallback is private. A patch segment that names it is E0340, the message says the part is private, and the note lists only the public names. A final `*` skips private names, and a scope whose only names are private declares no name for `*`. `@_name` still resolves in the body. Names in the entry file stay public, because every patch there is written in that file. Every name used to be a patch target, so renaming a part that a def names only for `copy @part` broke callers.
  - An unnamed node has no address. `verb~N` is not a patch target.
  - Patches apply in source order, and the last value set wins. When an outer scope and an inner one both reach one node, the outer one wins. A patch's named args are merged over the node's own. `off` drops the node.
  - A hop through a group or a layer is E0340 at the patch. The fix names the child and keeps the copy indices, so `.face.leaf` becomes `write .leaf`, `.face.0.leaf` becomes `write .leaf.0`, and `.face.*` names a child rather than `.*`. Dropping the segment used to lose the index, and a star became `.*`. Turning an instance or a slot off does not skip this check. A later hop is checked in the scope it enters, so `.hero off` does not hide `.hero.head.typo` and `.hole off` does not hide `.hole.typo`. An empty slot fallback declares no names, so `.hole.foo` cannot match a `foo` in the surrounding file. A hop that names nothing declared is E0340, and the note lists the region's names. When the segment matches the verb of an unnamed statement, the note cites that statement. The longest verb wins, so `.a1` cites an unnamed `a1` and not an earlier unnamed `a`. A count after the verb cites that unnamed draw, so `.circle2` cites the second unnamed `circle`. The fix is `add name:` at that statement's file and line. A name that is declared but not drawn, because of `off`, an untaken `if`, or an empty `each`, is not E0340. A drawn family whose index matches no drawn copy is E0340, including an index that only an undrawn arm of an `if` would have used. The fix names a drawn copy. A patch whose target is an anchor is E0340, and the fix says to patch a shape or a group. Each patch is reported once, at its line. A `play` reports a miss at the `play`, once per play, anim, and path.
- **Slots.**
  - `slot name [at:]` emits the caller's fill content, or its own block if there is no fill, at that z position, transformed by its `at:`. The slot frame is the enclosing frame's size, centered on the slot origin.
  - A slot's content is its own scope, keyed under the slot. Two fills of two calls may each name a node `form`. The flat table used to put every fill in the caller's table, so one fill's name could shadow another's.
  - Slot content resolves `@x` in its own names, then the host, then the scope the fill was written in. A slot's own block was written in the def, so that third place is the host again.
- **`needs:@a`.** A def body sees its own names plus `needs:` imports, and it does not see the caller. `needs:@a` is one name in the host, not a dotted path. A missing host, or a missing part, is E0311 at the call, and the body is not evaluated. The note lists the host's names. The old check ran only when a host existed, so an instance outside a slot skipped it.
- **Component values.**
  - A param whose value is a Component can be used as a verb.
  - Inside a body, `pick([tunic apron])` returns a Component.
  - `none` draws nothing.
- **`z:`.** Applied after the siblings are built: `behind` moves the node first, `front` moves it last, and a number sorts stably.
- **References.** `@` is lexical. A def used to see names from its caller, so the same def meant different nodes at different call sites.
  - The first segment of `@x.y` resolves in the first place that declares `x`.
    1. The current scope's names.
    2. In slot content, the host's names, then the scope the fill was written in.
    3. In a def body, its `needs` imports.
  - The same name in more than one of those frames is E0343. The note lists each frame. The current copy of a family is one match, not a second declaration. A slot, and a `needs:` name used from that slot, keep the host loop's current copy.
  - Later segments hop through instances and slots the way patches do, or name an anchor. The segments after that are measurements.
  - A family is every copy, except inside the loop that makes it, where it is the current copy. A consumer that needs one node and gets the family is E0343.
  - Ref geometry and bounds are stored in canvas coordinates and mapped into the current local coordinates by the inverse of the current transform.

## 10. Checks

- Checks are collected and evaluated when their enclosing scope (an instance body or the file) finishes. All measurements are in canvas coordinates.
- Bounds are visual: geometry plus half the stroke width. `.geom.*` uses geometry only.
- Functions:
  - `level(a b)` compares `cy`.
  - `gap(a b)` is the separation along the axis where the boxes don't overlap: horizontal if they overlap vertically, otherwise vertical. It is 0 if they intersect.
  - `dist` is the center distance.
  - `symmetric(a b about:x)` checks that `a.cx + b.cx == 2x` and that the two are level.
  - `about:` defaults to the parent's frame center x.
  - `inside(a b margin:)` checks that a's bounds sit within b's bounds shrunk by the margin.
  - `same_size(list)` uses `tol:` (default .5).
  - `@row.*` expands to the named nodes whose tree parent is `row`. When `row` is a family, that is the named children of every copy. `@row.0.*` lists only copy 0. When `row` is an instance or a slot, those are the named nodes at the top of its body or content.
- `~=` and `same_size` use `tol:` (default .5).
- Failure codes are C0101 for `assert`, which is an error, and C0201 for `expect`, which is a warning. Each failure has notes with the measured values and expectations.
  - When one node's position explains the failure (`level`, `aligned`, `symmetric`, `inside` overflow on one side), add `fix: set .<address> at:<x>,<y>` with the corrected local position. The address reaches the measured node from the scope the check is written in. When that node's key is not stable, the fix is `add name:`. When the key is stable but it is not a suffix of the check's scope, the fix uses the full key, as in `set .host.eye at:x,y`. The old fix could name `.eye` while the check had measured a different copy, or it said `add name:` for a node that already had one.
- `rule` is a parameterised check bundle. `assert rule_name(args)` runs its checks with the params bound, and failures report both locations.

## 11. Targets, manifest and build

- A file with one or more `@target` directives is an entry file. A file with none is a library.
- A directive after another statement is E0101. `@target` lines come first.
- `use` of an entry file is E0201.
- Two declarations that produce the same output path, after matrix expansion, are E0202. The note names the first declaration.
- `@target` under a manifest lib root is E0203.
- A drawing statement at a library's top level is E0204. Allowed forms there are `use`, `palette`, `style`, `finish`, `def`, `rule`, `let`, and `anim`.
- The out path is relative to the manifest `out`. `scale:` applies to `.png` only, where scale 1 is `name.png` and scale k is `name@kx.png`. A `.gif` is one file at scale 1. `scale:` does not add `@2x` copies.
- `bg:` defaults to the manifest `[defaults] bg`, which defaults to `none` (transparent).
- **Backend.** A build draws on the GPU (Skia Ganesh on Metal) or on the CPU raster backend. `[build] backend = "auto" | "gpu" | "cpu"` in `artscript.toml` picks it, and `auto` is the default. `artscript build --backend auto|gpu|cpu` overrides the manifest. A build resolves the choice once. `auto` is the GPU when this machine has one and the CPU otherwise. `gpu` with no GPU is E0400 and nothing is built, with no fallback to the CPU. Only macOS has a GPU backend today. Any other value in the manifest is E0200 at that line. Any other value for `--backend` prints usage and exits 1, like any unknown flag. SVG elements do not use the backend. A baked PNG tile, a PNG, and a GIF frame do. A surface the GPU cannot create, such as one past the maximum texture size, is E0400.
- `art-manifest.json` sits in `out`. It holds `{ "version": 1, "outputs": [ { "path", "source", "line", "target", "matrix", "scale", "hash", "status": "pass|warn|fail", "checks": [..] } ] }`, sorted by path.
  - Hashes are FNV-1a 64 of the output bytes, as hex.
- **Rebuild.** Delete files listed in the old manifest that the new build no longer produces. Never touch unlisted files.
- **Cache.** `.artscript/cache/<target-key>.json` maps the cache key to the output hashes. The key hashes:
  - the entry source
  - every transitively imported library source
  - the manifest
  - the resolved backend, so switching backends, or `auto` resolving differently, re-renders
  - the hash of the `artscript` executable, so a renderer change re-renders every target
  - the target's matrix values, `fps`, `dur` (or `auto`), `clip` (or `none`), and `t`
  
  When the key matches and the outputs on disk match their hashes, skip the target.
- **Determinism.** The same source, manifest, and resolved backend produce byte-identical outputs on the same machine.
  - GPU output can differ from CPU output and between machines. `backend = "cpu"` keeps every build on the CPU renderer, which avoids GPU differences between machines.
  - Seeds come from FNV-1a over the project-relative file path, `\0`, and the node's key (section 4), plus `seed:`. The old text hashed the positional path. That path changed when a group was wrapped around a named node.
  - Keys are built from the source text and loop indices only, never from an evaluated value, so two builds of one source produce the same keys.
  - Floats in SVG print with at most 3 decimals and no trailing zeros.
  - There are no timestamps.

## 12. Not implemented in v0.1

- `shader`, `light` and `brush` parse, and evaluating them reports E0390 "not yet implemented in v0.1". The message names the phase, as in `shader (Phase 2)`.
- `fx bevel` and every other Phase 2 effect report E0391.
- Rendered checks (`contrast`, `visible`) report E0392.

## 13. Details the other sections leave out

These record behavior the sections above do not mention. When this section disagrees with another one, choose the better behavior and make them agree. Follow the opening of this file when you resolve it.

- **Imports.**
  - A plain `use x` binds every export of `x` by name. `use x { a b }` binds only those names. `use x as m` binds only `m`.
  - A name starting with `_` is not an export. Both forms skip it, and `use x { _name }` is E0212.
  - `m.name` on a module alias falls back to the module's palette entries, so `p.rim` works through the default palette alias.
  - A `use` that names a project-relative entry file (`use art/b`) is E0201, the same as one that resolves through a lib root.
  - Top-level `let`s of a module are visible inside its `def`s.
  - Finishes are imported the same way as styles. `use x` and `use x { surface }` bind them by name. `use x as m` does not, and there is no `fx m.surface` form.
  - A style and a finish may share a name. Both of those forms bind both.
- **Node identity.**
  - Ordinals are static. One `each` emits `circle~1.0`, `circle~1.1`. A second `each` drawing `circle` in the same block is the next ordinal, so its copies are `circle~2.0`, `circle~2.1`. Both arms of an `if` count, so toggling an `if` does not renumber a later sibling. The old count bound the `each` variable to the first item on a separate pass, and `pick` could rename a node between that pass and the draw.
  - `each part in [eye nose]` drawing `part` is `part~1.0`, `part~1.1`. The verb is the word written, whatever component it holds.
  - Only named nodes are registered for `@` and patches. A bare `circle` is not addressable, and neither is an unnamed instance. A check inside that instance can still measure `@self`. The record stays hidden until the body finishes, so a draw that reads `@self` is E0350. The record is not entered in the caller's name table.
  - A key with a `~` is positional. When a node draws a seed from a positional key, `positional-seed` (L0104) reports each unnamed statement on that key once, at that statement's line.
- **Patches.** When an outer scope and an inner one both patch the same node, the outer one wins. E0340 is decided by the names a scope declares, not by the nodes drawn on this frame. An instance or a slot that is `off` still admits the rest of a patch that hops through it. An empty slot fallback is an empty scope. Matching the drawn frame used to hide a typo under a hidden parent.
- **Seeds.**
  - `rand`, `pick`, and noise fields draw from a seed of the file and the node's key, plus a counter that advances on every call in that statement. The statement line is not part of that seed. Moving the draw keeps the same result.
  - `jitter:`, `blob`, and `fx grain` seed from the node's key with their tags.
  - A `let`, `if`, `each`, or `play` statement seeds from the enclosing node's key, the loop indices since that node, and its tag among its siblings: `let y~1`, `if~2`, `each i~1`, `play bob~1`. The tag counts the same way as an unnamed draw's `verb~N`, through both `if` arms and once per loop body, and each child block counts from 1. A comment, a blank line, or `artscript fmt` keeps every seed. The old seed used the line, so any of those moved a `let y = rand(...)`.
  - `fx` argument expressions keep their current seeding. This change does not touch them.
- **Repeat and grid.**
  - `repeat n rot:r step:x,y` applies `i * rot` and `i * step` to copy `i`.
  - `grid` copies are centered on the grid's origin. The cell size is `cell:` or the bounds of the first copy, plus `gap:`. Primitives inside a grid still default to the frame center, so the whole grid sits at the frame center.
- **Paint.**
  - A closed shape with a tapered stroke draws only the tapered outline, in the stroke paint, with no fill.
  - When a tapered stroke's width list has a different length from the curve's points, the widths are spread evenly along the curve.
  - `circle 50%` takes the percent of `min(w, h)` of the frame.
  - `shadow` and `inner` with no offset use `0,4` and `blur:4`.
  - The fx default colors use the step formula on the resolved color. They do not use the node's ramp index.
- **Measurements.** `.area` is the bounds area, `w * h`.
- **Lints.**
  - `raw-color` only scans statements. `@target` keys such as `bg:` are exempt.
  - `unused` (L0103) covers `let` bindings, def parameters other than `needs` and `size`, and imported names. A `play` name counts as a use of that import. `off-canvas` (L0102) covers shapes that are not clipped by a parent shape.
  - `positional-seed` (L0104) warns by default. It fires when `rand`, `pick`, a noise field, `jitter:`, `blob`, or `fx grain` hashes a key that has a `~`. It reports each unnamed statement on that key once, at that statement, and the fix is `add name:`. Set it to `off` or `error` in `[lint]`. It is aggregated across frames the same way as `off-canvas`, one diagnostic per site. A throwaway that nothing addresses and that does not feed a key-derived seed stays unnamed and warns nothing.
  - A lint set to `error` makes `build` exit with 2, the same as a failed `assert`.
  - A lint level in `[lint]` other than `off`, `info`, `warn` or `error` is E0200 at that line.
- **Codes added in this version.**
  - E0200: the manifest is missing or invalid, or a CLI argument is invalid. A manifest error points at the line and column of the bad value or of the TOML syntax error. A `project`, `defaults`, `lint` or `build` key that is not a table is E0200. A bad `[defaults] profile` is reported at its line.
  - E0206: a bad `@target` size, scale, extension, profile, matrix axis, `fps`, `dur`, or `t`. A profile is one of `vector`, `svg`, `raster`, or `auto`, on every format. `profile:vector` or `profile:svg` on a `.png` or `.gif` target is E0206 (`` `profile:vector` needs an .svg target ``). A manifest `profile` of `vector` or `svg` still applies only to `.svg` targets, and `.png` and `.gif` targets draw as raster. Any other manifest `profile`, a non-string value included, is E0200 at that line in `artscript.toml`, reported once. Also a GIF with no duration, a GIF whose background is missing or not opaque (`a gif needs an opaque bg:`), a GIF with more frames than centiseconds (`a gif has more frames than centiseconds`), a GIF frame longer than 65535cs (`a gif frame is longer than 65535cs`), a GIF whose frames would exceed 256MiB of RGBA (`a gif raster is larger than 256MiB`), a GIF whose retained scenes would exceed 256MiB (`a gif retains more than 256MiB`), or a clip with more than 100000 frames (`a clip has more than 100000 frames`). The zero-delay fix says to lower `fps` or lengthen `dur`. The long-frame fix says to raise `fps` or shorten `dur`. The raster fix says to lower `fps`, shorten `dur`, or shrink the canvas. The retained-scene fix says to lower `fps`, shorten `dur`, or draw less in each frame.
  - E0308: `play` names an anim that is not in scope.
  - E0320 also covers a value that cannot be animated, a bad `key` (`dur`, `at`, `ease`), a non-positive duration, and a `key` outside an anim with no `dur:`. The message is `cannot animate <kind> yet` when the kind has no interpolation.
  - E0341: an `anim` body contains something other than a patch line, or the anim has no indented patch block.
  - E0342: `play` is not at the top level of an entry file and not under an instance.
  - E0311 also covers an instance whose def has `needs` and that sits in no slot. It is reported at the call, and the body is not evaluated.
  - E0312: a `name:` that is not one identifier, a second name on a node that already has one (`layer hat name:cap`), or a call whose key is not stable and whose def contains a name. A bad `name:` is reported once, at that `name:`. The fix is `add name:` or one identifier `name:`.
  - E0333: a name used twice in one scope, outside exclusive arms of one `if`. Reported at the second name, with a note naming the first line. The fix says to rename one.
  - E0340: a patch segment names nothing declared in its scope, a final `*` in a scope that declares no name, a segment before the last names a node that is not an instance or a slot, a segment names a private `_` part, a copy index matches no drawn copy, an index is not a whole number, or the target is an anchor. A `play` reports it at the `play`, and the message names the anim and the full path, so `.left.typo` and `.right.typo` are both reported. A patch expression or blend failure is reported on that patch, in the anim's file. The fix is `write .leaf`, `write .leaf.0`, `add name:` at the unnamed verb, or `patch a shape or a group`.
  - E0343: a reference, patch, or check that resolves to several nodes where one is needed. The note lists the spellings.
  - E0344: an `if` or `each` around a top-level `play` takes a different branch or item count before the scene is drawn than while it is drawn, such as a guard that reads `@name.w`. It is reported once per `if` or `each`. A top-level play's `start:` or anim expression that reads the drawn scene is E0344 too.
  - E0207: an unknown `{var}` in a target path.
  - E0208: an unknown directive. Every file reports it, including a file with no `@target` and a library under a lib root.
  - E0209: a `profile:vector` target that uses raster-only features. The notes list each node.
  - E0210: a module that cannot be found.
  - E0211: an import cycle.
  - E0212: an imported name that the module does not export.
  - E0304: `palette x from y` where `y` does not exist. E0305: a `color` row whose value is not a color, or a `ramp` row without one or two colors.
  - E0306: `each` over something that is not a list or range, or more than 100000 iterations. `[each k in ... -> ...]` uses the same rule.
  - E0307: an unknown style.
  - E0322: a percent measured against an empty frame. Inside a def, the message names the def.
  - E0334: `slot` outside a `def`.
  - E0336: `of:` or `as:` on a line inside a finish.
  - E0337: a finish named after a built-in effect.
  - E0338: a finish line that is not a built-in effect, such as a nested finish.
  - E0339: `finish:` used as a node key.
  - E0350: an unknown reference or member. The notes list the names that exist.
  - E0351: measuring a reference that has no geometry.
  - E0400: a render or write failure.
- **Build.**
  - The cache file name is the FNV-1a hex of the target key (`source:line[matrix]`).
  - A target whose evaluation reports an `E` error writes no outputs. Its previous manifest entries are kept with `status: fail`, so the old outputs are not deleted.
  - With `--only`, entries of targets that were not selected stay in the manifest unchanged.
  - The manifest's `checks` field lists the `C` diagnostics as `"<code>: <message>"`, and appends ` (<node>)` when the diagnostic has a key. `E` and `L` diagnostics stay out of that field.

## 14. Time, anims and GIFs

- **Time.** `ms` and `s` are suffixes. A bare number in a time position is seconds. A boolean is not a time. `anim` `dur:`, `play` `start:`, and `key`'s `dur:` reject it with E0320. `fmt` prints a positive sub-second whole number of milliseconds as `Nms`, and every other time as seconds. `0.5s` prints as `500ms`. `1500ms` prints as `1.5s`.
- **`t` and `clip`.** `t` reads the evaluator's time. While an anim's body is sampled, that time is the anim's local time. `clip` reads the target's clip name, or `none`.
- **`key`.** Fractions are `i / (n - 1)`. One value returns that value. `at:` is fractions of the key's duration, non-decreasing, in `[0, 1]`. A boolean in `at:` is E0320. The length must match the values. A zero-width segment uses the later value at that instant. The default duration is the anim's `dur`. Outside an anim, a missing `dur:` is E0320. `dur` must be positive.
  - The default interpolation is uniform Catmull-Rom with the endpoint tangents duplicated. `ease:` replaces that curve. `linear` is `s`. `in` is `s^2`. `out` is `1 - (1 - s)^2`. `in_out` is `2s^2` below 0.5 and `1 - (2 - 2s)^2 / 2` otherwise. `hold` keeps the left value until the next key.
  - A non-looping key holds the first value before 0 and the last value after `dur`. `loop` uses the fractional part of `t / dur`.
  - Numbers, angles, vectors, and sizes interpolate in value space. An angle is a number of degrees, so `key(0 90deg)` is one kind. An angle does not take the short arc. Colors interpolate in OKLab, with alpha linear, then the usual gamut map. Words, components, and booleans step and ignore `ease`. A percent, a vector or size of percents, a ramp, gradient, list, string, field, or reference is E0320 at every sample, including the endpoints and `ease:hold`. Mixed kinds in one key are E0320. A vector and a size are different kinds.
- **`anim`.** The duration is evaluated in the anim's module and seeded from the anim's file, the play's owner key, and the anim's name. A duration that fails or is not positive is reported at the anim, with a note naming the `play`. The body is an indented block of patch lines (E0341 when the block is missing or holds something else). Anims are not values. `use` copies them the way it copies styles, skipping `_` names. `at` and `rot` add to the rest value. A percent rest position is measured against the node's frame before a numeric offset is added. A percent offset still adds to a percent rest. `scale` multiplies. `skew` and `pivot` are set. `abs(v)` on `at`, `rot`, or `scale` sets that channel. `name:` on a patch or an anim channel is E0320. The node's name stays the name on the draw. A patch expression is seeded from the anim's file, the play's owner key, the anim's name, and the patch path with its count among the anim's patches to that path (`.box~1`). `style:` is expanded after static patches and anim channels, so the last `style:` word supplies the paint keys. A word applied by an anim is resolved in that anim's module, and a word that module does not declare is reported at that patch. A word on the node or a static patch is resolved in the node's module. An explicit paint key on the node, a patch, or a channel still beats that style's key. `off` hides the node and still counts as a match. A patch line with only a path sets nothing, and it is E0340 when it matches nothing. A channel under a hidden node still matches, because E0340 checks declared names, and a typo there is still E0340. A `*` is allowed only as the last segment.
- **`play`.** Allowed at the top level of an entry file and under an instance (E0342). The entry file's top level is the instance those paths patch. Static patches set the rest pose. Plays then apply in order of position. A play's position is its line in the entry file, after the line and copy index of each enclosing `each`, `repeat`, or `grid`. So plays apply in source order, and the plays of one `each` iteration apply before those of the next. This holds for a play under an instance, under an instance in a structural draw such as a `group`, and under an `if` or `each` that reads geometry. A play under an instance in a `def` body has no position in the entry file. It applies in the order the instances are drawn. When it and an entry-file play reach one node, the entry-file play wins, because the outer scope wins (section 9). Offsets accumulate. The last set value wins. `play none` records nothing.
  - A `let` at file top is a module global from that line onward. A `let` inside `if` or `each` stays in that block, including under an instance. A `let` written as a direct line under the instance is visible to later plays, `fx`, and `mask` in that call, and not after the call. A play under an instance is scheduled while the scene is drawn, so its guards may read geometry. A top-level play is scheduled by a prepass, before anything is drawn. The prepass reads those bindings, then restores the module globals, so a later `let` is not visible to an earlier line. As it walks, it records each named draw in the entry file's scope, including a name inside a `group` and each `repeat` or `grid` copy, so a later `if @name` or `if @name.0` sees that name. It skips the draws the drawing pass does not register, such as a component bound to `none`, and a draw whose verb is a `let` the prepass cannot compute. It still records a draw that a top-level play hides with `off`. The drawing pass does not register that draw on a frame where it is hidden, so an `@name` of it there is E0350. The record has no geometry, and the names inside an instance are not recorded. An `if` or `each` that holds a top-level `play` is evaluated in the prepass and again while drawing. When the two passes take a different branch or a different item count, it is E0344 at that `if` or `each`. So a guard that reads `@name.w`, or a `let` computed from it, is an error, not a dropped play. A guard that fails in both passes reports only the drawing pass's error. A guard with no top-level `play` under it, other than `play none`, is not checked. The prepass cannot read the drawn scene. A member of a node such as `@name.w`, `@name.part`, or `@name.*` fails there, and so do a check function such as `gap`, a rule whose check reads one, and an `@name` that is declared but the prepass has not recorded. An `@name` that nothing declares is E0350, as it is while drawing. A copy index such as `@name.0` is read from the recorded copies. A `let` computed from one fails the same way wherever that binding is visible, including through `.{name}` and a call such as `name(…)`. A later binding of the same name, such as an `each` variable or a rule parameter, is a new value. A top-level `start:`, or an anim expression that play evaluates, that reads one is E0344, reported where that expression reports its other errors. `canvas.w` and `@canvas.w` are known before drawing. Errors from that prepass's `let`, `if`, and `each` are dropped, because the real pass reports them. Errors from the `play` itself are kept.
  - Local time is `t - start` (`start` defaults to 0, evaluated at the `play`). A `start:` expression is seeded from the play's file, the owner key, and the play's tag, then the previous seed is restored. A looping anim is always live and wraps with `rem_euclid`. A non-looping anim is live only on `[0, dur)`. A path that matches nothing is E0340 at the `play`, once per `play`, anim, and path, including when this frame is outside the window.
  - The clip duration, when `dur:` is omitted, is the latest `end` among plays reached at `t = 0`. A clip loops when any play reached on a sampled frame has an anim that loops.
- **Sampling.** `sample` is the entry point for a time-based target. Frame `i` is at `t = i / fps`, over `[0, dur)`, for both looping and non-looping clips. A product `dur * fps` that lands within `1e-8` of an integer does not grow the boundary frame, so `100ms + 200ms` at 10 fps is three frames. A count above 100000 frames is E0206 and `sample` returns no frames. Checks run on every frame. One failure on one node is reported once, with notes `first at {t}s` and `worst at {t}s`. The same check on another node stays a separate failure. Two applications of one rule stay two failures when their `via rule … applied at` notes name different call sites, and two plays that reach one bad anim duration stay two failures when their `played at` notes differ. The worst frame is the greatest absolute number found in the failure's notes, ignoring notes that start with `via rule` or `played at`, otherwise the greatest failing `t`. The message, notes, and fix are that worst frame's. `first at` stays the earliest failing time. `off-canvas` (L0102) is aggregated the same way, one diagnostic per node. A node that is clipped on one frame and entirely outside on a later frame stays one failure, and the worst frame supplies the message. Other lints stay on the first frame. `check` evaluates a still once. A GIF target goes through `sample`, so a check that fails on a later frame is reported. `sample` also reports a clip with no duration, a delay of 0, a delay above 65535cs, a GIF whose declared background is missing or translucent, a GIF side longer than 65535px, and a GIF whose `width × height × frames × 4` exceeds 256MiB, the same E0206 `build` uses before it encodes. Before it keeps a frame, `sample` charges the heaviest scene so far against the frames still to come. A clip whose retained scenes would pass 256MiB is E0206 and `sample` returns no frames, so `check` reports it before those scenes accumulate. Each frame's diagnostics are folded into the aggregate as the frame is evaluated. A pixel with alpha below 255 is still found when the GIF is encoded.
- **GIF.** One NeuQuant palette, `sample_fac` 10, built from every frame. Delays are centiseconds. The total is `round(dur * 100)`, spread with Bresenham so 24 fps over 1 second is `4,4,4,4,4,5`. A delay of 0, or a delay above 65535cs, is E0206 and no GIF is written. A side longer than 65535px is E0206. Before any frame is rendered, `width × height × frames × 4` is capped at 256MiB. A larger raster is E0206 and nothing is written. The scenes held for those frames are capped at 256MiB as well. A heavier clip is E0206 and nothing is written. Parallel GIF encodes share the raster cap and wait, so several encodes cannot each hold a full raster at once. The scene cap is per target. A looping clip repeats forever. Any other clip plays once. A missing background, a background alpha below 1, or any pixel with alpha below 255 is E0206.
