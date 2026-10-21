# Design note: a shading language ramose owns

Status: accepted for #307. Not implemented. `docs/topics/brushes.md` describes the SkSL brushes the code runs today, and it stays true until #308 ports them.

## Why ramose needs its own shading language

A brush body today is SkSL that the author writes by hand. That ties every brush in `std/brushes`, the catalog, and the examples to Skia in three ways:

- Skia has no 3D pipeline, so a 3D renderer cannot run a brush.
- The author sees Skia's split between a runtime shader and a runtime blender. A body that reads `m.below` compiles as a blender, and every other stage of its stack does too.
- SkSL owns the error messages. The brush crate keeps a line map from the assembled source back to the `.ramo` file, and it rejects a parameter whose name is an SkSL keyword or builtin.

The decision on 2026-10-21 was a small typed language that ramose owns. It shades one surface sample that 2D and 3D share, and it compiles to SkSL now and to WGSL later. ramose ships no closed brush set and no public SkSL.

## A body is ramose lines over one sample

A brush body is a block of ramose lines under the `brush` line. The `sksl """..."""` child goes away. The body uses the expression syntax and the operators from `values`, so an author learns one language.

```
brush bevel toward:-1,-1 depth:7 strength:.7
  let k = dot(m.edge_normal normalize(toward))
  let band = 1 - smoothstep(0 depth m.edge)
  let tint = if k > 0 then (1,1,1) else (0,0,0)
  out lerp(m.paint.rgb tint abs(k) * band * strength)
```

The body has three kinds of line:

- `let NAME = EXPR` binds a value that later lines read. A `let` cannot reuse the name of an earlier `let` or of a brush parameter.
- `fn NAME(a b) = EXPR` declares a helper. It follows the rules of a top-level `fn`: one expression, no recursion, and a parameter wins over any other name. A helper sees its parameters, the brush's parameters, `m`, and the other helpers. It does not see the body's `let` lines, just as a top-level `fn` does not see its caller's locals.
- `out RGB cover:NUMBER` is the last line, and the body has exactly one. `RGB` is a vec3 of straight rgb. `cover:` is the coverage in 0..1. With no `cover:`, the coverage is the surface's own antialiased coverage: `cover(m.edge)` on a 2D sample, and 1 on a 3D sample, where the rasterizer owns the edge.

The output word is `out` and not `shade`, because `shade(c k)` is already a color builtin. The parser gains these three lines inside a `brush` block. Today `fn` opens a declaration only at the top level.

A body can also call a top-level `fn` from its own file or from a `use` import, so `dry` and `chip` can share a helper in one library. The checker checks that `fn` as body code. From a body it may read its parameters and other fns, but no top-level `let` and no palette. An error that does not depend on the argument types, such as an unknown name, is reported once at the `fn`. A type error that one set of argument types causes is reported once at the first call with those types, with a note at the `fn`. The same rule holds for a helper in the body.

`m` is the surface sample. The head keys keep their meaning. `dir`, `reach`, and `on` set placement, and every other key is a uniform that the body reads by name. A body reads no palette and no top-level `let` directly. It takes them as parameters, as `tint:p.ink` does today.

## The surface sample has shared fields and 2D extras

A brush that reads only shared fields runs on every surface. Reading a 2D extra makes the brush 2D-only. The 2D sample's frame is x right, y down, and z toward the viewer, so it is left-handed. #322 maps that frame into a 3D scene, and #256 gives `light.dir` in it.

### Shared fields

| field | type | 2D fill | 2D stroke and ring | 3D surface |
|---|---|---|---|---|
| `position` | vec3 | the pixel in the node's local units, z = 0 | the same | the point on the surface in the object's local units |
| `normal` | vec3 | `(0,0,1)` | `(0,0,1)` | the unit surface normal in the object's local frame, on the side the camera sees |
| `uv` | vec2 | today's `uv`: `position.xy` relative to the geometry's center, rotated so x runs along `dir` | `(s,d)`, so x starts at the contour's start | the surface's own coordinates in local units, centered on the surface (#298 defines them) |
| `size` | vec2 | the size of the rotated bounds along and across `dir` | the contour's arc length, and the full width at `s` | the extent of the surface's `uv` |
| `paint` | color | the straight rgba the pixel would have had, or the previous stage's result | the same, from the stroke's paint | the same, from the surface's paint |
| `seed` | number | the stage's seed, 0..255 | the same | the same |
| `px` | number | local units per device pixel | the same | local units per device pixel at the point, from the projection |
| `light` | light | the scene light from #256, in the sample's frame | the same | the same |

The units and the meaning of each field match across surfaces. The origin of `uv` does not. A ribbon's `uv.x` is arc length from the contour's start, and every other surface centers `uv`. `size.y` on a stroke follows the width at `s`, and on every other surface it is constant.

`light` is a record. `light.dir` is the unit vec3 from the point toward the light, and `light.color` is a color. #256 fills it and may add members. Until #256 lands, a read of `m.light` is an error.

### 2D extras

| field | type | 2D fill | 2D stroke and ring |
|---|---|---|---|
| `edge` | number | signed distance to the geometry's edge in local units, positive inside | the half width at `s` minus the distance to the spine, positive inside |
| `edge_normal` | vec2 | today's `m.normal`: the unit vector out of the shape at the nearest edge, `0,0` on a medial axis | the unit vector from the spine to the pixel, `0,0` on the spine |
| `s` | number | distance along `dir` from the near edge of the rotated bounds | arc length along the spine, past the ends below 0 or above `size.x` |
| `d` | number | offset across `dir` from the center | offset across the spine, positive on the right of travel |
| `pressure`, `angle`, `tilt`, `velocity` | number | 1, `dir`, 0, and 1 | the node's channels at `s` |
| `below` | color | the straight rgba already drawn under the pixel in the node's group | the same, with the node's own fill included |

The rules for `s`, `d`, the channels, and `below` stay as `brushes` states them today. `below` stays raster-only, as #312 decided. A brush that reads it cannot run on an `.svg` tile, on an `fx outline` ring, in a 3D renderer, or in an engine export. A brush that reads a 2D extra cannot run on a 3D surface. The section on errors says where each failure is reported.

### Fields that go away

- `m.p` becomes `m.position.xy`.
- `m.normal` moves to `m.edge_normal`, so `normal` can mean the surface normal on every surface.
- `m.len` and `m.width` become `m.size.x` and `m.size.y`.
- `m.u` and `m.v` go away. A body that wants them writes `m.s / m.size.x` and `m.d / (m.size.y / 2)`.
- `m.dev` goes away. No brush in the repo reads it, and shading by device pixel ties a brush to the output resolution (#310). The frame keeps the device pixel for its dither.
- `seeded(m)` goes away. Noise seeds itself, as a later section describes.

## Types are inferred and fixed per body

A value in a body has one of six types: number, vec2, vec3, color, bool, and light. A list exists only as the argument of a reduction (see the section on loops). The checker infers the type of each `let` once. It instantiates each helper for the argument types of each call and inlines it. A parameter's type comes from its default: a number is a number, a color is a color, and a vector is a vec2 or a vec3 by its length. A vec3 default needs three-component vectors in `values` (#321).

- `+ - * /` work elementwise on vectors. A number combined with a vector applies to each component, so `c + 7.3` adds 7.3 to every component of `c`.
- A color takes no arithmetic. `.rgb` reads its straight rgb as a vec3 of sRGB-encoded values, as SkSL brushes see it today. `.r .g .b .a` read one component.
- `.x .y .z` read a vector, and `.xy` reads the first two components of a vec3.
- A tuple joins numbers and vectors into a vec2 or a vec3, so `(q,z)` lifts a vec2 `q` and a number `z` into a vec3. A tuple with more than three components is an error.
- `if c then a else b` takes a bool and two values of one type.

A body has no mutable state, no recursion, and no texture read other than `m.below`. Two runs with the same sample and the same uniforms give the same result.

### Builtins keep their ramose meaning

A math builtin that `values` has keeps its meaning in a body. The noise names are the one exception, described after the table. #308 gives the evaluator the same elementwise forms, so `floor((1.5,2.5))` works in a declaration too. A name that is not in this table, such as `rand`, `pick`, or `polar`, is an error in a body.

| builtin | arguments | notes |
|---|---|---|
| `abs floor round fract sqrt sin cos exp` | a number or a vector | per component. `sqrt` clamps a negative input to 0, and `round` rounds half away from zero, as in `values` |
| `min max` | two numbers or two vectors, or one list of numbers | per component, or a reduction over the list |
| `sum` | one list of numbers | a reduction |
| `clamp lerp step smoothstep pow mod` | numbers or vectors, and a number argument applies to every component | `mod` is floored, as in `values` |
| `lerp mix` on two colors | a color, a color, and a number | both mix in OKLab and map the result into gamut, as in `values`. A port of today's SkSL `mix` uses `lerp` on `.rgb` |
| `atan2 rad` | numbers | `atan2(y x)` returns degrees. `rad(deg)` converts degrees to radians |
| `dot length normalize` | vectors | |
| `rot(p deg)` | a vec2 and degrees | the exact rotation from today's prelude |
| `cover(d)` | a number | a signed distance in local units to a coverage antialiased over one pixel |
| `hash hash2 value simplex worley` | a vec2 point | 0..1, or a vec2 for `hash2`. Self-seeding, see below |
| `fbm(p oct:n)` | a vec2 point, and `oct:` from 1 to 8 with a default of 4 | fbm of `simplex`. `oct:` must be a constant. A value outside 1..8 is an error, never a clamp |

`value`, `simplex`, `worley`, and `fbm` share their names with `fields`, but not their signatures. In a field fill, `simplex(40)` takes a feature size and `fbm` wraps a field. In a body, `simplex(p)` and `fbm(p)` take a point, and a number argument is a type error whose fix writes `(x,0)`. `hash` and `hash2` exist only in a body. The two forms of a name stay apart until field fills compile to this language (see the last section).

A constant is a number literal, or arithmetic on number literals. A `let`, a parameter, and a field of `m` are never constants, even when they hold a literal.

`lerp` and `mix` on two colors need the OKLab conversion and the gamut map from `crates/eval/src/color.rs` in the prelude. That code is float arithmetic outside the exact set, like `simplex`. No existing brush needs it.

### Loops are bounded comprehensions

A body has no `for` and no `while`. It loops with the list comprehension from `values`, and only as the direct argument of `min`, `max`, or `sum`:

```
let hit = max([each y in -1..2 -> max([each x in -1..2 -> flake(floor(g) + (x,y) g depth)])])
```

The bounds of a range must be constant numbers, such as `-1..2`. A brush parameter is a uniform, so it cannot bound a range. A range must hold at least one item. Nested comprehensions in one expression multiply, and the product of their item counts is at most 256. A break of any of these rules is an error at the comprehension. The backend prints each comprehension as a loop with constant bounds, which SkSL runtime effects and WGSL both accept.

## Noise seeds itself from the stage

Each stage has a seed derived as today, from the node's key, the slot, and the stage's index. `seed:` on the node still pins every stage. The seed gives two values, also as today. `m.seed` is the seed mod 256. The noise offset is `(seed mod 61, floor(seed / 61) mod 59)` and has 3599 values.

- A noise call adds the stage's offset to its point. So `fbm(m.uv / 45 oct:3)` differs between two nodes with no `+ seeded(m)`.
- `salt:k` on a call uses the offset of a second seed that mixes the stage's seed with `k`. Two calls at one point with different salts give unrelated noise in every node. `k` must be a whole-number constant.
- `seed:n` on a call uses the offset of `n`, so the noise is the same in every node, as `seed:` is on a field fill. `seed:0` is the offset `(0,0)`. `n` must be a whole-number constant.

Render computes every offset on the CPU from integers, once per stage and salt, and binds it as a uniform. The shader never derives a seed. The exact lattice hash, `m.seed`, and the 3599 offsets stay as `brushes` describes.

## Errors are reported where the author can fix them

The checker owns the language rules. It reports each error once, at the `.ramo` file, line, and column that caused it. `ramose check` runs the whole checker without Skia. The error codes stay in their families:

- E0360 covers an error inside a body: a type mismatch, an unknown name, a builtin not available in a body, a loop rule, or a read of `m.light` before #256. An `sksl` child is E0360 at the `sksl` keyword, and its fix names the new body. That brush reports no second error for its missing `out` line.
- E0361 covers a brush call, E0363 a parameter, and E0364 a `brush:` value, as today.
- E0362 and E0209 cover a brush that cannot run where it is used, as today. The error is at the use: the `brush:` key, the `fx` line, the node for an `.svg` profile, or the key that puts the brush on a 3D surface. A note points at the read of `m.below` or of the 2D extra. A library brush that nothing misuses reports nothing, and each misuse reports once.

A backend that fails on a checked body is a bug in ramose. It reports one internal error at the `brush` line and names the backend.

## One typed IR feeds every backend

The compiler has one front end and one backend for each target:

1. The `syntax` crate parses the body lines.
2. The checker builds a typed expression graph for each body. It records which `m` fields the body reads, whether the body reads `below`, and whether the body is 2D-only.
3. A backend prints the graph as one function of the sample, then wraps it in the frame for its target. The frame binds the uniforms, builds the sample, applies the paint's alpha, premultiplies, dithers, and snaps the result to 8-bit levels.

The backend gives every author name a prefix. So a parameter called `half` or `flat` becomes legal, a `let` called `float` cannot break the target, and no author name can reach the runtime's `u_` uniforms. A name in a body still cannot be a field of `m`, a builtin from the table above, or a ramose keyword such as `t` or `in`. A name that `values` has but a body does not, such as `line`, is free in a body. The checker owns the language, so the line map from SkSL back to the body goes away. The text scan that finds `u_` names and reads of `m.below` goes away too.

Each builtin lowers to code with its `values` meaning, so `sqrt` and `round` emit guarded forms and not the target's own.

### The SkSL backend runs today's frames

The SkSL backend prints the graph as `float4 shade(Mark m)` and reuses the shader frame and the blender frame from `crates/brush`. Render picks the frame per stack, as today. When any stage reads `below`, every stage runs as a blender. The author never sees the split.

The lattice hash, `rot`, the dither, and the 8-bit snap stay exact on CPU and GPU. `simplex`, `value`, `worley`, `fbm`, and the OKLab conversion are float arithmetic. So a GPU build can differ from a CPU build by a level or two at some pixels, as today. Each backend stays byte-identical with itself on every build.

### The WGSL backend serves 3D and other renderers

#309 adds a WGSL backend with the first 3D renderer. It prints the same graph as a WGSL function and ports the prelude. Every intermediate of the lattice hash is an integer below 2^24, so the hash stays exact in WGSL f32. #309's test compares the two backends within a tolerance. It uses a brush that reads only shared fields and does not wait for #256, such as:

```
brush facing ambient:.3
  out m.paint.rgb * (ambient + (1 - ambient) * max(m.normal.z 0))
```

Noise over a vec3 point needs a 3D lattice hash with the same integer rules. #309 specifies it. Until then, a noise call in a body takes a vec2 only.

## Every brush in the repo ports to the new body

#308 ports the SkSL in these files:

- `crates/eval/std/brushes.ramo`, `catalog/art/brush/*.ramo`, and `examples/spire/lib/paint.ramo`.
- The tests in `crates/eval/src/tests.rs`, `crates/eval/src/tests/brush.rs`, `crates/render/tests/brush.rs`, `crates/render/tests/below.rs`, `crates/render/tests/reach.rs`, `crates/render/tests/ribbon.rs`, `crates/render/tests/warp.rs`, `crates/syntax/tests/fixtures/mvp_brush.ramo`, and `crates/syntax/tests/syntax.rs`.
- `fuzz/art.dict`.

Each SkSL form in use has a form in the new body:

| SkSL in use | brushes | new form |
|---|---|---|
| straight math with `smoothstep`, `clamp`, `pow`, `exp`, `sqrt` | all | the builtin of the same name |
| `mix` on `float3` colors | most | `lerp` on `.rgb` vec3s |
| `float3(v, z)` and `float3(1)` | `sculpt`, `form` | `(v,z)` and `(1,1,1)` |
| a local that changes several times, such as `c` | `form`, `sculpt` | one `let` for each value |
| a helper function | `dry` (`pass_load`), `chip` (`flakes`) | `fn` in the body |
| a fixed 3x3 loop with an `if` | `chip` | nested comprehensions with `if then else` and `max` |
| a ternary | `bevel` | `if then else` |
| `hash12`, `hash22`, `vnoise`, `fbm(p, n)`, `cover(d, m)` | all noise users | `hash`, `hash2`, `value`, `fbm(p oct:n)`, `cover(d)` |
| a local named like a keyword, such as `t` | `dry` | another name |
| `atan(y, x)` in radians | `sculpt` | `rad(atan2(y x))` |
| `m.p`, `m.len`, `m.width`, `m.u`, `m.v`, `m.normal` | most | the renamed fields above |
| `+ o` with `o = seeded(m)` | all noise users | the default self-seeding |
| `+ o * 2`, `o * 3`, `o * 1.3` | `bristle`, `chip`, `sculpt`, `ink` | `salt:1`, `salt:2`, and so on |
| `m.seed + 4` as a noise coordinate | `bristle` | the same coordinate with `seed:0`, so no offset is added |

`chip`'s flakes are the hardest case. They need helpers and a bounded loop:

```
brush chip strength:.6 scale:1 grit:.5
  fn local(c g) = rot(g - c - hash2(c) hash2(c).x * 360)
  fn square(r c) = (max(abs(r.x) abs(r.y) * 1.7) - .35 - .55 * hash2(c).y) * 9 * scale
  fn flake(c g depth) = if hash(c + 7.3) < strength then cover(-square(local(c g) c)) * cover((.35 + .65 * hash(c + 2.1)) * depth - m.edge) else 0
  let depth = strength * 9 * scale
  let g = m.position.xy / (9 * scale)
  let hit = max([each y in -1..2 -> max([each x in -1..2 -> flake(floor(g) + (x,y) g depth)])])
  let e = m.edge - depth * .25 * value(m.position.xy / (1.4 * scale))
  let near = 1 - smoothstep(0 depth * 1.5 + 2 m.edge)
  let pit = smoothstep(.78 .82 value(m.position.xy / (1.1 * scale) salt:1)) * near * grit
  out m.paint.rgb * (1 - .15 * near * strength) cover:cover(e) * (1 - hit) * (1 - pit)
```

A helper is one expression, as a top-level `fn` is, so a long helper splits into small ones. `local` and `square` take the place of the SkSL locals `r` and `dd`. The flakes keep today's pixels. The offset is a whole number, so adding it inside the hash gives the same lattice value as today's `hash22(floor(m.p / C + o) + ...)`.

The port changes pixels only where a salt replaces a hand-made `o * k` offset. That is `chip`'s pits, `bristle`'s fine streaks, and some noise in `sculpt` and `ink`. `dry` and `form` read only `+ o`, so they keep their pixels. #308 reblesses the goldens that draw `chip` or `bristle` and names each one. It also rewrites the header of `catalog/art/brush/normal.ramo` to say `m.edge_normal`.

When #308 lands, it rewrites the sentences that this design leaves false:

- The SkSL sections of `docs/topics/brushes.md`.
- The brush bullet in `docs/language.md`, which says a brush shades "with SkSL".
- The header comment of `crates/eval/std/brushes.ramo`.
- The brush sentence in `README.md` and the brush line in `plugins/ramose/skills/ramose/SKILL.md`, which both say SkSL.
- The builtin rules in `docs/topics/values.md`, which gain the elementwise forms, `.xy`, the vector tuple, and `rad`.

## Alternatives considered

| option | why not |
|---|---|
| keep public SkSL | no 3D, Skia's shader and blender split in the author's view, and errors in Skia's terms |
| a closed set of brushes with parameters | an author cannot write a new look, and `catalog/art/brush/` and `examples/spire` declare brushes of their own today |
| a C-like language in a `"""` block, close to GLSL | agents and shader authors know the syntax, but ramose would carry a second grammar, a second set of names that disagree with `values` (radians, `mix`), and a second error path |
| a node graph | a text language is what an author writes in a `.ramo` file and what an agent edits |

The C-like option was the closest call. An agent writes GLSL from memory and has to learn `min(a b)` with spaces. ramose lines win because an author already writes them everywhere else in the file, the checker already reports their spans, and a math builtin means the same thing in a body and in a declaration. Only the noise names differ, until field fills compile to this language.

## Left for later

- #308 ports the brushes and removes `sksl """..."""`.
- #309 adds the WGSL backend and the 3D lattice hash.
- #256 defines `m.light`.
- #298 defines `uv` and `size` on a 3D surface that ramose builds.
- #321 allows the three-component vector defaults that vec3 parameters need.
- #322 maps the 2D sample's frame into a 3D scene.
- #325 uses the shared fields to put a 2D component on a 3D surface as its material.
- A field fill such as `fill:fbm(simplex(30)) -> p.stone` is already a small expression over a sample in `eval`. It can compile to the same IR, so a field fill and a brush share one backend. That needs IR nodes for `perlin`, `ridged`, `wrap:`, and `fbm` over any field. No issue tracks that change yet.
