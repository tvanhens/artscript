# artscript language card

This card describes the draft the code implements today. The name of the draft is v0.1. The design is still open. A better design can replace any rule here, and it can break today's outputs. When you change a rule, update this card and `docs/semantics.md` in the same change.

`docs/semantics.md` records the details that this card leaves out. When the two files disagree, choose the better behavior and make them agree.

artscript is a declarative language for vector art. `artscript build` turns every entry file into PNG, SVG, and GIF. One statement per line: `verb positional… key:value…`, children indented two spaces. `#` starts a comment.

A line that does not draw says so with its first word: `use palette style finish def rule let shader light brush anim` declare, `if else each` branch and loop, `assert expect` check, `slot` exposes a slot, `play` starts an anim, `ramp color` are palette rows, `fx` adds an effect, `mask` masks, `.part` patches, and `name:` at the end of a line fills a slot. Every other line is a draw: a shape, a structure verb, or a component. `fx`, `mask`, `slot`, `finish`, `anim`, and `play` are reserved, so `let fx = 1` is an error.

## Files

- An **entry file** starts with one or more `@target` lines. Any file without one is a **library** and may only hold `use`, `palette`, `style`, `finish`, `def`, `rule`, `let`, `anim`.
- `@target produce/peach.png 1024x1024 scale:1,2` writes `peach.png` and `peach@2x.png`. `.svg` targets write SVG. `.gif` targets write one GIF at scale 1; `scale:` does not add `@2x` copies or change the stroke sample. Keys: `bg:none|<color>`, `as:<label>` (the file reads it as `target`), `profile:vector|svg|raster|auto` (`vector` and `svg` need an `.svg` target, and an unknown word is an error), `fps:` (default 24), `dur:`, `clip:` (the file reads it as `clip`, default `none`), `t:` (the time a still draws, default 0), `each k:[a b]` fans out a matrix interpolated as `{k}` in the path. `fmt` prints `clip:` after `each`. `bg`, `scale`, `as`, `fps`, `dur`, `t`, `clip`, and `profile` are never axes, so `clip:[idle blink]` stays a `clip:` error. A GIF needs an opaque `bg:`, at most one frame per centisecond of `dur`, and at most 256MiB of raster and 256MiB of retained frames.
- `use face/eye` imports a library from the manifest's lib roots. `use palette/dusk as p` aliases it. `use face/parts { eye blush }` imports names. Names starting with `_` stay private.
- The manifest's default palette is always available as `p`.

## Values

| kind | examples |
|---|---|
| number | `12`, `.5`, `-3` (canvas units) |
| percent | `50%` of the current frame (the canvas, a parent shape's bounds, or a def's `size` box) |
| angle | `30deg`, `.25turn`, bare numbers in `rot:` are degrees |
| time | `200ms`, `2s`, a bare number in a time position is seconds |
| vector / size | `14,0` (no spaces around the comma), `256x256` |
| color | `#ff5e5b`, `oklch(.78 .07 55)`, `p.skin.3`, `p.skin` (a ramp's base) |
| list / range | `[a b c]`, `0..8`, `[each k in 0..n -> expr]` |
| reference | `@body`, `@hero.head.eye_l`, `@body.neck` (anchor), `@self`; also `canvas` |

Expressions: `+ - * /` (elementwise on vectors; a number scales a vector, as in `(dx,dy)*.28`, `2*v`, `v/2`; `-v` negates), `== != < <= > >= ~=`, `and or not`, `in lo..hi` (`lo` included, `hi` excluded), `if c then a else b`. Write binary operators with spaces on both sides or neither: `w/2`, `a + b`. `-x` after a space starts a new argument. Functions: `min max abs clamp lerp sqrt floor round sin cos atan2 mod` (degrees; `atan2(y x)`, floored `mod(a b)`), `polar(r deg)`, `rand(lo hi)`, `pick(list)`, and the color helpers below. `v.x`, `v.y` read a vector.

## Color

```
palette garden
  ramp peach #f3a2a6 5        # 0 deep, 1 shadow, 2 base, 3 light, 4 highlight
  ramp leaf #7db956 5 hue:12  # hue: degrees of shift per step
  ramp sky #1b1f3a #f2a65a 7  # two endpoints, interpolated in OKLCH
  color ink #3d2632
```

A palette holds only `ramp` and `color` rows. `ramp <name>` takes one or two colors, an optional stop count (default 5), and an optional `hue:`. `color <name>` takes one color. Outside a palette, `ramp` and `color` are ordinary names, so `color:p.ink` and `ramp(#fff 3)` still work.

A single-color ramp darkens toward cool blue-violet and lightens toward warm yellow, so shadows and lights are hue-shifted for free. `p.peach.line` is the ramp's lineart color, darker and more saturated than the base. `palette night from garden` inherits and overrides.

- `shade(c k)` steps `k` along `c`'s ramp: `-1` shadow, `-2` deep, `+1` light, `+2` highlight.
- `mix(a b t)` mixes in OKLab, `fade(c a)` sets alpha, `line(c)` is the lineart color for any color.
- Gradients interpolate in OKLCH. `grad(down a b)` runs across the shape's bounds, with directions `down up left right` or an angle (0 = right, 90 = down). `grad(300,240 720,440 a b)` uses those two points as the start and end in the shape's local coordinates. Use `radial(a b at:.3,.3 r:200)` (`at:` is a fraction of the bounds, `r:` is in units) or `conic(a b c)`. A stop can carry a position, as in `p.peach.3,.4`.

## Shapes

Shapes are centered on their local origin, and `at:` places that origin in the parent.

| shape | example |
|---|---|
| rect | `rect 40x20 r:4` (or `r:4,4,0,0` per corner) |
| circle, ellipse | `circle 12`, `ellipse 20,12` (radii) |
| poly, star | `poly 6 r:20 round:3`, `star 5 r:20 inner:.5` |
| curve | `curve 0,0 20,-15 40,0`, or `curve pts` when `pts` is one list of points. `closed` fills, `tension:.5`. `round:r` on a closed curve at `tension:0` |
| blob | `blob 30 seed:4 wobble:.2` organic closed shape |
| path | `path "M0 0 c10-20 30-20 40 0z"` |
| copy | `copy @body at:-20,-8` reuses a named node's outline |
| booleans | `subtract`, `union`, `intersect`, `xor` with shape operands as children, first minus the rest |
| mirror | `mirror x` draws its block and the block flipped across x = 0 |
| repeat, grid | `repeat 8 rot:45`, `grid 4x3 gap:8` (`i`, `row`, `col` in the block) |

Any node takes `at: rot: scale: skew: pivot: a:` (opacity), `blend:` (`multiply screen overlay soft_light add erase` …), `z:behind|front|N`, `name:`, `style:`.

Paint keys:

- `fill:<color|gradient|field>`.
- `stroke:<color>,<width>`. Strokes have round caps and joins. `stroke:3` alone uses `line(fill)`. A range is not a width. `stroke:p.ink,2..6` is an error, and the fix is `stroke:p.ink,[2 6]`.
- Tapered ink: `stroke:p.ink,[2 8 3] taper:.3` on an open curve gives one width per point along its length, with ends thinning over 30%. `taper:0,.6` sets the start and end separately.
- `blur:6` softens a shape's edge.
- `jitter:1.5,60` wobbles the outline like a hand-drawn edge. The pair is the amplitude and the wavelength.

## Structure

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
- `fx` lines apply to the draw or `def` they sit directly under, in order. They cannot go at the top level, in a palette, or directly under `if` or `each`. A draw inside an `if` still takes its own `fx`. Use `blur 4`, `shadow 0,6 blur:8`, `inner 0,4 blur:4` (a shadow cast inward from the top edge), `glow 8`, `rim 3 dir:1,1` (a light band on that side), `adjust hue:10 sat:-.2 light:.05`, `outline 4` (a line around the shape geometry behind the node, default `line(fill)`, unchanged by an earlier blur, shadow, or glow), and `grain .05 scale:1.5`. Each takes `color:` and `a:`. `as:name` names a result and `of:name|@node|self` picks an input, and `merge a b a:.4,1` combines them.
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

## Fields

`simplex(40)`, `perlin`, `worley`, `value` take a feature size, `seed:` and `oct:`. Wrap them with `fbm(f)` or `ridged(f)` and combine them with arithmetic. Use `fill:fbm(simplex(30)) -> p.stone` to map a field through a ramp or a list of colors, or a `mask worley(20) > .6` child line to threshold one into a shape. Seeds default to a hash of the file and the node's key. A named node keeps its seed when other nodes change. An unnamed node's seed can shift when a sibling with the same verb is added ahead of it, and `positional-seed` warns. Add `name:` to pin it.

## Components

```
def eye size:10 iris:p.sky.3 look:0,0
  circle size fill:#fff name:white
    circle size*.55 at:look*size*.3 fill:iris
  anchor brow 0,-size
  slot lashes at:0,-size*.8
```

- The first parameter is positional (`eye 9 at:-14,0`), and types come from the defaults.
- `size` is the def's box, centered on the origin. `size:40` is a square. `size:72x32` is 72 wide and 32 tall. A bare percent uses the smaller side. Percents in the body measure that box, and a child with no `at:` stays at the origin.
- Components are values, so `outfit:tunic` can be placed as `outfit`, and `none` draws nothing. A def may reuse a shape word. `def circle` is that component, not the built-in circle.
- `slot name` exposes an insertion point, filled by the caller with a `name:` block. A def body may hold `fx`, which attach to every instance and run before the call site's. A mask, a slot fill, or a patch cannot sit directly in a def body. Slot content is its own scope. `@part` there searches the content, then the host, then the caller. A def body sees only its own names. `needs:@torso` imports the host's `torso`, and a missing host or a missing part is an error at the call.
- `.arm_r rot:-20`, `.cape off`, and `.head.eye_l iris:p.accent` override a declared name from the instance block, in source order, with the last one winning. Every segment before the last is an instance or a slot, so `.head.eye_l` needs `head` to be one. `.eye.0` reaches one copy of a family. A segment may be a keyword when the part is a slot of that name, as in `.mask off`. Hiding the instance with `off` still checks a hop through it.
- A part whose name starts with `_`, as in `name:_core`, is private to its def. `@_core` works in the def body. A patch or an anim path from outside the def that names `_core` is an error, and `.*` skips it. The entry file has no caller, so its own `_` names stay patchable.
- `let w = if build == slim then 34 else 42`, `if`/`else` (a statement `if` takes a block, not `then`), and `each x in [..]` / `each i in 0..8` are bounded control flow. `each i in 0..8` runs 8 times, with `i` from 0 through 7. `[each k in 0..n -> expr]` builds the list.
- `style ink stroke:p.ink,2` with `style:ink` applies those keys to a node and its descendants. A style takes keys only, never a block. A style holds keys that children inherit, while a finish holds effects that one node gets.

## Animation

`t` is the clip time in seconds. Inside an `anim` it is that anim's local time. `clip` is the target's `clip:` name, or `none`.

```
key(0 6 0)
key(0 1 1 0 at:[0 .3 .7 1] ease:in_out)
anim idle dur:2s loop
  .face at:0,key(0 6 0)
  .leaf rot:key(-4 4 -4)
anim blink dur:200ms
  .eye lid:key(.6 1 .6)
play idle start:0s
```

- `key` samples the current time. Values are spaced evenly unless `at:` lists fractions from 0 to 1. The default curve is smooth. `ease:` is `linear`, `in`, `out`, `in_out`, or `hold`, one name or one per segment. `loop` wraps. Outside an anim, `dur:` is required.
- Numbers, angles, vectors, and sizes interpolate. Colors interpolate in OKLab. Words, components, and booleans step. Anything else is an error.
- `anim` is a declaration. Its body is patch lines. `at` and `rot` add to the rest pose. A percent rest `at` is measured against the node's frame before a numeric offset is added. `scale` multiplies. `abs(v)` on those three sets the value. Every other argument is set, except `name:`, which stays on the draw. A patch expression is seeded from the anim's file, the owner key, the anim's name, and the patch's path, so a comment or a blank line keeps its value. A `style:` word from the anim is resolved in the anim's module. `play` is a statement at the top of an entry file or under an instance. `play none` does nothing. Plays apply in source order after static patches. A play under an instance in a `def` body applies in the order the instances are drawn, and an entry-file play that reaches the same node wins over it. A top-level `play` is scheduled before anything is drawn, so its `start:`, its anim's expressions, and an `if` or `each` around it cannot read geometry such as `@name.w` (E0344). The last set value wins. A `let` inside `if` or `each` under an instance stays in that branch. A direct `let` under the instance is visible to later plays there.
- A named node's key ignores layers and groups, so `layer body` holding `name:robe` is `robe`. An unnamed node's key is `verb~N` under its parent. `explain` prints the key, including nodes drawn in a mask block. A `~` means the node is not addressable. A node that drew a seed from its key is marked `seed`.

## Checks

Checks measure the evaluated scene and never move anything. An `assert` failure is an error, and an `expect` failure is a warning.

```
assert level(@eye_l @eye_r) tol:.5
assert inside(@hero canvas margin:24)
expect @head.w / @body.w in .9..1.4 "head as wide as the body"
```

Measurements are `@a.x .y .w .h .cx .cy .left .right .top .bottom .area`. Functions are `inside(a b margin:)`, `overlaps`, `touches`, `gap`, `dist`, `aligned(a b edge:top)`, `level`, `centered(a in:b)`, `symmetric(a b about:x)`, `aspect`, `on_grid(a n)`, `same_size(@row.*)` and `stroke_widths(a)`. `@row.*` lists the named children of every copy. Inside a loop, `@dot` is that copy, including from a slot in the loop and from a second loop beside the one that draws it. A check inside a def can use `@self` when the call has no `name:`. A draw that reads `@self` is an error. `rule name subject:@self` + checks bundles them and is applied with `assert name(@x)`.

## CLI

`artscript build [--only 'glob'] [--allow-warnings] [--backend auto|gpu|cpu]`, `check [--json]`, `fmt [--check] [FILE...]`, `explain <file>`, `init`, `docs`. `docs` prints this card. With no files, `fmt` rewrites every `.art` file. `fmt --check` lists files that would change and writes nothing. `check` evaluates each still once and samples each GIF, and it does not render. An unknown flag on any command prints usage and exits 1. Exit codes: 0 ok, 1 compile error or a `fmt --check` diff, 2 an assert failed, 3 only warnings.
