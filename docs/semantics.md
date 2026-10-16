# artscript v0.1 implementation semantics

The design doc is `MVP.md`, and the author-facing reference is `docs/language.md`. This file settles the details both leave open. When they disagree, this file wins for v0.1.

## 1. Lexing

- **Lines.** A statement is one line. The only multi-line token is a `"""` raw string. Blank lines and comment-only lines are skipped and do not affect indentation.
- **Indentation.** Indentation is spaces only, and a tab in indentation is error E0102. A line indented deeper than the previous statement opens a block under it. A dedent must return to an enclosing level, otherwise it is E0103 and the note names the nearest valid parent line.
- **Comments vs colors.**
  - `#` followed by exactly 3, 4, 6 or 8 hex digits and then a delimiter (whitespace, end of line, `,`, `)`, `]`) is a color.
  - Any other `#` starts a comment that runs to end of line.
- **Numbers.**
  - Numbers look like `12`, `1.5`, `.5`. There is no exponent, and the sign is never part of the token.
  - The suffixes `%`, `deg` and `turn` attach with no space. A percent is stored as 0.5 for `50%`, and a turn as 360 degrees.
- **Sizes.** A number or percent, then `x`, then a number or percent with no spaces is one size token: `256x256`, `100%x60%`, `8x40`. `mirror x` stays an identifier because `x` is not glued to a number.
- **Identifiers.** Identifiers match `[A-Za-z_][A-Za-z0-9_]*`. Keywords are `use as from palette style def rule let shader light brush assert expect if then else each in and or not true false none`, plus the reserved list in `lib.rs`. A reserved word used as a name is E0105.
- **References.** `@` followed by an identifier, then `.segment` repeats where a segment is an identifier, digits or `*`.
- **Member access.** A `.` glued to the end of an identifier, a `)` or a reference continues a member chain (`p.sky.3`, `v.x`). A `.` at the start of a statement followed by an identifier is a patch.
- **Strings.** `"..."` supports the escapes `\" \\ \n`. `"""..."""` is raw and may span lines. Its common leading indentation is stripped.
- **Whitespace.** Every token records whether whitespace preceded it, and the parser needs this.

## 2. Parsing

The first tokens of a line decide its form:

| line starts with | form |
|---|---|
| `@ident` before any statement | directive. `@target` takes a raw path token (everything up to whitespace), then args, named args, and an optional `each` followed by named axes |
| `use` | `use <raw path> [as x] [{ a b }]` |
| `palette`, `style`, `def`, `rule`, `let`, `shader`, `light`, `brush` | declarations. In `def` and `rule` every parameter is a named arg. `def` accepts `needs:@a` or `needs:@a,@b` as a parameter named `needs` |
| `assert`, `expect` | `expr named* "message"?` |
| `if`, `each`, `else` | control. `else` must directly follow an `if` block at the same indent. `else if` chains are allowed |
| `.ident` | patch. Path segments, named args, and an optional trailing `off` |
| `ident:` then end of line | slot fill, which needs a block |
| any other identifier | node: the verb, then items |

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

- A node's path is its parent path, a `.`, and its name.
- The name is resolved in this order:
  1. `name:`
  2. a layer's or group's positional name
  3. for an instance, the def name
  4. otherwise the verb
- A second unnamed sibling with the same base name gets a 1-based suffix, so the second is `circle2`.
- Mirror copies append `_m`.
- Repeat, grid and each copies append `#<i>` to every top-level node of the copy.
- Slot content lives under `<instance path>.<slot name>`.

## 5. Values and types

- **Runtime values** are Num, Vec2, Size, Percent, Color (which remembers its ramp and index if it came from one), Ramp, Palette, Paint (gradient), Field, Range, List, Str, Ident (enum-like words such as `slim`, `down`, `closed`), Component, Ref, Bool and None.
- **Def parameter types** are inferred from their default. Passing a different kind is E0320, and the fix names the expected kind.
- **Ramp as a color.** A ramp used where a color is expected is its base stop.
- **Identifiers** resolve in this order: `let` and loop bindings, def params, the `target` constant and matrix variables, palettes and aliases, components, and finally bare enum words.
- **Arithmetic.** `+` and `-` on two vectors add and subtract. `*` and `/` on two vectors are elementwise. A number on either side of `*` scales a vector, and `v/n` scales by `1/n`. Unary `-` negates a vector. A pairing that is none of those is E0320.
- **`atan2(y x)`** returns degrees, the same unit as `sin`, `cos`, `polar` and `rot:`. **`mod(a b)`** is the floored remainder (`mod(-1 3)` is 2). A zero divisor is E0320.

## 6. Color

- Implement OKLab and OKLCH conversion as in `../chibi/.cursor/skills/chibi-art/toolkit/src/chibi/color.py`.
- Out-of-gamut colors are mapped by reducing chroma (binary search), never by clipping.
- **Single-color ramp `ramp c n [hue:h]`.**
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
- **Two-color ramp `ramp a b n`.** Interpolate in OKLCH, taking the shorter hue arc.
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
- **Tapered ink.** A stroke whose width is a list or range, or a stroke with `taper:` set, becomes a filled outline (`Shape.fill = stroke paint`, no stroke). Use the `brush` algorithm from chibi `paths.py`:
  1. Sample the curve every 0.75 output pixels. In the path's own units that step is `0.75 / (transform scale × target scale)`. The transform scale is the accumulated scale (the longer axis of the local-to-canvas matrix). The target scale is the target's highest `scale:`, so `@2x` stays as smooth as `scale:1`. A stroke in unit space under `scale:50` then matches the same stroke written in pixels.
  2. Interpolate the per-point widths by chord fraction.
  3. Taper each end with the floor `.12` and ease `sin(x*pi/2)^.8`.
  4. Add round caps.
  - A range `a..b` is two widths, and a single width with `taper:` is that width everywhere.
- **Curve.** Use centripetal Catmull-Rom with tension (default .5), exactly as `_spline` in chibi `paths.py`. `closed` closes the curve. Points are the positional arguments. When the only positional argument is a list, its items are the points (`curve pts`, `curve [each k in 0..n -> x,y]`). A list beside other points is E0320. There is no separate polygon-of-points verb: a closed curve is that polygon.
- **Blob.**
  - Use `points:` (default 7) points on a circle of radius r.
  - Each radius is scaled by `1 + wobble * u` with `u` in -1..1 from the seed, and each angle is jittered by `±wobble * 0.5 * step`.
  - The result is joined with a closed curve.
- **Poly and star.** The first vertex points up. `round:r` rounds corners with Skia's corner path effect, then resolves the result to a path. A closed `curve` at `tension:0` is straight sides, and `round:r` rounds those corners with the same effect. A smooth curve has no corners, so `round:` is read only at tension 0.
- **Booleans.** Use PathOps across the operands' transformed geometries, in the boolean node's local coordinates.
- **`copy @ref`.** Takes the referenced node's silhouette (the union of its shape geometries, transformed into current local coordinates).
- **`jitter:amp,wavelength`.** Apply chibi's `wobble`, seeded from the node path. The pair is the only spelling: a bare number would hide the wavelength, and a second key would split one setting. A bare number is E0320, and the fix writes the amplitude with wavelength 60, which is what older art meant. Wavelength must be positive.
- **`blur:` on a shape** sets `Shape.blur`.

## 8. Effects (`fx`)

- `fx` is a child statement and is appended to the parent node's `effects`. The statement `fx <op> positional… named…` maps onto `EffectOp`.
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
- `grain` is a 3-octave Perlin overlay, in the style of chibi `pen.grain`. Octave weights are 1/2, 1/4, and 1/8. `scale` is the finest cell size in canvas units (default 1.5). The seed comes from the node path. Overlay opacity is `min(1, 2 * strength)`, so strength .05 swings a mid gray by 5% at the peaks. Chibi divides the octave sum by that crop's peak. The shader divides by 0.58, the peak of this noise on a flat shape.
- `outline` traces the node's shape geometry and draws it behind the content. Earlier `blur`, `shadow`, and `glow` do not change the traced path. The areas are the filled geometry of the subtree. A shape's children are omitted, because they composite source-atop and do not change its alpha. A stroke contributes that stroke's filled path. Each area is filled and stroked at `2 * width` with round cap and join, in the outline color.
- `as:` sets `Effect.name`. `of:` takes `self`, a name, or `@node`.

## 9. Components, slots and overrides

- **Calling a def.**
  1. Bind its params (positional first, then named).
  2. Collect the instance block. It may contain slot fills, patches, `fx`, and `mask:`. Anything else is E0330, and the fix suggests wrapping it in a slot fill.
  3. Evaluate the body in a fresh scope whose frame is the instance box. That box is the resolved `size`, centered on the origin. A def with no `size` keeps an empty frame.
- **Patches.**
  - Patches are applied in source order, and the last value set wins.
  - When the instance body creates a node whose path relative to the instance equals a patch path, the patch's named args are merged over the node's own.
  - `off` drops the node.
  - Patches with longer paths are forwarded into nested instances with the prefix stripped.
  - A patch that matches nothing is E0340, and the note lists the part names.
- **Slots.**
  - `slot name [at:]` emits the caller's fill content, or its own block if there is no fill, at that z position, transformed by its `at:`. The slot frame is the enclosing frame's size, centered on the slot origin.
  - Slot content is evaluated with the caller's lexical scope plus host lookup. `@x` searches the host instance's named nodes created so far first, then outward.
- **`needs:@a`.** When the def is instantiated as slot content, the host must have a part `a`. Otherwise it is E0311, listing the host's parts.
- **Component values.**
  - A param whose value is a Component can be used as a verb.
  - Inside a body, `pick([tunic apron])` returns a Component.
  - `none` draws nothing.
- **`z:`.** Applied after the siblings are built: `behind` moves the node first, `front` moves it last, and a number sorts stably.
- **References.** `@name` resolves by walking scopes outward:
  1. the current instance's named nodes so far
  2. the slot host's named nodes
  3. enclosing instances
  4. file top level
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
  - `@row.*` expands to the named children.
- `~=` and `same_size` use `tol:` (default .5).
- Failure codes are C0101 for `assert`, which is an error, and C0201 for `expect`, which is a warning. Each failure has notes with the measured values and expectations.
  - When one node's position explains the failure (`level`, `aligned`, `symmetric`, `inside` overflow on one side), add `fix: set .<node> at:<x>,<y>` with the corrected local position.
- `rule` is a parameterised check bundle. `assert rule_name(args)` runs its checks with the params bound, and failures report both locations.

## 11. Targets, manifest and build

- Follow MVP.md "Targets" rules 1 to 7 and the codes E0201 to E0204.
- The out path is relative to the manifest `out`. `scale:` applies to `.png` only, where scale 1 is `name.png` and scale k is `name@kx.png`.
- `bg:` defaults to the manifest `[defaults] bg`, which defaults to `none` (transparent).
- `art-manifest.json` sits in `out`. It holds `{ "version": 1, "outputs": [ { "path", "source", "line", "target", "matrix", "scale", "hash", "status": "pass|warn|fail", "checks": [..] } ] }`, sorted by path.
  - Hashes are FNV-1a 64 of the output bytes, as hex.
- **Rebuild.** Delete files listed in the old manifest that the new build no longer produces. Never touch unlisted files.
- **Cache.** `.artscript/cache/<target-key>.json` maps the cache key to the output hashes. The key hashes:
  - the entry source
  - every transitively imported library source
  - the manifest
  - the hash of the `artscript` executable, so a renderer change re-renders every target
  - the target's matrix values
  
  When the key matches and the outputs on disk match their hashes, skip the target.
- **Determinism.**
  - Seeds come from FNV-1a over the project-relative file path, `\0`, and the node path, plus `seed:`.
  - Floats in SVG print with at most 3 decimals and no trailing zeros.
  - There are no timestamps.

## 12. Not implemented in v0.1

- `shader`, `light` and `brush` parse, and evaluating them reports E0390 "not yet implemented in v0.1" with the design-doc phase.
- `fx bevel` and every other Phase 2 effect report E0391.
- Rendered checks (`contrast`, `visible`) report E0392.

## 13. Clarifications from implementation

These pin down points the sections above leave open. Earlier sections still govern.

- **Imports.**
  - A plain `use x` binds every export of `x` by name. `use x { a b }` binds only those names. `use x as m` binds only `m`.
  - `m.name` on a module alias falls back to the module's palette entries, so `p.rim` works through the default palette alias.
  - A `use` that names a project-relative entry file (`use art/b`) is E0201, the same as one that resolves through a lib root.
  - Top-level `let`s of a module are visible inside its `def`s.
- **Node identity.**
  - The sibling suffix counter is keyed by the base name plus any `#i` suffix. So the copies of one `each` are `circle#0`, `circle#1`, and a second loop's copies are `circle2#0`, `circle2#1`.
  - Only nodes with `name:`, named layers and groups, and instances are registered for `@refs`. A bare `circle` is not addressable.
  - Slot content registers its names in the caller's table, and resolves `@x` against the host first.
- **Patches.** When an outer instance and an inner one both patch the same node, the outer one wins.
- **Seeds.** `rand` draws from a seed of the file, node path, `#i` suffix and statement line, plus a counter that advances on every call in that statement.
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
  - `unused` (L0103) covers `let` bindings, def parameters other than `needs` and `size`, and imported names. `off-canvas` (L0102) covers shapes that are not clipped by a parent shape.
  - A lint set to `error` makes `build` exit with 2, the same as a failed `assert`.
- **Codes added in this version.**
  - E0200: the manifest is missing or invalid, or a CLI argument is invalid.
  - E0206: a bad `@target` size, scale, extension, profile or matrix axis.
  - E0207: an unknown `{var}` in a target path.
  - E0208: an unknown directive.
  - E0209: a `profile:vector` target that uses raster-only features. The notes list each node.
  - E0210: a module that cannot be found.
  - E0211: an import cycle.
  - E0212: an imported name that the module does not export.
  - E0304: `palette x from y` where `y` does not exist. E0305: a malformed palette entry or `ramp`.
  - E0306: `each` over something that is not a list or range, or more than 100000 iterations. `[each k in ... -> ...]` uses the same rule.
  - E0307: an unknown style.
  - E0322: a percent measured against an empty frame. Inside a def, the message names the def.
  - E0334: `slot` outside a `def`.
  - E0350: an unknown reference or member. The notes list the names that exist.
  - E0351: measuring a reference that has no geometry.
  - E0400: a render or write failure.
- **Build.**
  - The cache file name is the FNV-1a hex of the target key (`source:line[matrix]`).
  - A target whose evaluation reports an `E` error writes no outputs. Its previous manifest entries are kept with `status: fail`, so the old outputs are not deleted.
  - With `--only`, entries of targets that were not selected stay in the manifest unchanged.
  - The manifest's `checks` field lists the `C` diagnostics as `"<code>: <message>"`.
