artscript — Design Doc
Oct 15, 2026 · @Tyler van Hensbergen
Overview
artscript is a small, declarative language for vector art that LLMs write and one command (artscript build) turns into SVG and PNG assets for apps and games. It trades general-purpose power for three things models need: few tokens per image, defaults that look good without being specified, and reuse mechanisms that let a repository grow its own art vocabulary.
The problem. LLMs can emit raw SVG, but the results are verbose, fragile and usually flat. SVG spends tokens on syntax (tags, quotes, closing elements) and makes the model do coordinate math and control-point placement it is bad at. It has no notion of palettes, tone ramps or reusable rigs, so every asset is drawn from scratch and nothing stays consistent across a game's art set.
Goals
1. Token efficiency. The same image in at least 3x fewer tokens than hand-written SVG, measured on a benchmark corpus.
2. Taste by default. Palettes with tone ramps, hue-shifted shading, consistent line weights and smooth curves are the easy path, not extra work.
3. Full expressive range. Layers, masks, filters, booleans, procedural fields, shaders and scene lighting, rendered by Skia, enough to reach professional illustration and game-asset quality.
4. Repository-level reuse. Palettes, styles, components, anchors and slots let a project build its own libraries (characters, clothing, faces, tiles, icons) without the language assuming any of them.
5. One command. artscript build from the project root renders every target, deterministically and incrementally. The CLI is written in Rust and ships as a single binary per platform with Skia built in, so there is nothing else to install.
6. Clear entry points. A file that produces output says so with an @target declaration; library files never do.
Non-goals
• General-purpose programming: no I/O, no network, no unbounded recursion.
• Animation in v1 (the syntax reserves room for it).
• A WYSIWYG editor in v1.
• Photorealism or raster painting; raster is an output, never hand-painted input.
Design principles
Seven principles settle most syntax and feature debates; when two proposals conflict, the higher one wins.
1. Tokens are the budget. Every construct is judged by tokens per unit of visual result. One statement per line, indentation instead of closing delimiters, positional arguments for the one or two most common parameters, and keywords chosen to be single tokens in common tokenizers. Common English words beat cryptic abbreviations: circle is one token and needs no explanation, crc saves nothing.
2. Defaults carry taste. An unstyled shape should already look intentional: round joins, palette-aware strokes, smooth curves through points, warm-highlight and cool-shadow shading. The model specifies intent; the language supplies craft.
3. One obvious way. Each idea has one canonical spelling, enforced by artscript fmt. Models learn faster from consistent examples, and diffs stay small.
4. Deterministic. The same sources produce byte-identical outputs. All randomness is seeded, and default seeds derive from the file path plus the node's path so an edit elsewhere never reshuffles a texture.
5. Mechanisms, not domains. The language ships general tools (parameters, anchors, slots, path overrides, palettes as values). It does not know what a character, a hat or a tile is; those live in project libraries.
6. Everything is addressable. Every node has a stable path (hero.head.eye_l), so errors, previews, overrides and LLM edits can target one part without rewriting the file.
7. Errors are prompts. Compiler messages are written for a model to act on: location, cause, and a concrete fix.
Foundational decisions
artscript is built on Skia, and the decisions below are made now because they are one-way doors: reversing any of them later would change existing outputs or break existing files. Everything else in this doc can be added or changed later without that cost.
Decision
Choice
Why it can't wait
Rendering model
Skia's drawing model defines what a program means. SVG is one output, and features it can't express are baked or rejected.
Every primitive's meaning, and every file written against it, depends on this.
Output profiles
Each target declares profile:vector, svg or raster. A profile that can't express a feature either bakes it or fails with an error.
Changing it later would silently change what existing targets produce.
Committed renderer
Skia's CPU raster pipeline at a pinned Skia version. GPU rendering is for previews only.
Switching would change pixels in every committed asset.
Units and origin
Canvas units everywhere, including inside shaders. Canvases have a top-left origin; component definitions have a centered local origin.
Every coordinate in every library depends on it.
Hierarchical transforms
at:, rot:, scale: and skew: are relative to the parent. Anchors and pivots belong to a part and move with it, so rotating an upper arm carries the forearm and hand.
This is the forward-kinematics model poses and rigs will build on; switching to absolute positions would mean rewriting every library.
Override order
Path overrides apply in source order, and the last one to set a value wins. Overrides can reach any parameter of any part, including transforms, visibility (off) and draw order (z:).
Poses will be named, stackable bundles of overrides; blending them needs a defined order.
Pure evaluation
The scene is a pure function of the file, its parameters and t. Seeds derive from node paths, never evaluation order, and nothing persists between evaluations.
Animation evaluates the same file at many values of t; any hidden state would make textures flicker between frames.
Node paths as identity
A part's path (hero.body.arm_r) is its identity for overrides, poses, animation keys, checks and diagnostics. Renaming a part is a breaking change to its library, and the compiler never renames on its own.
Every external reference to a part depends on it.
Color pipeline
Author in OKLCH. Gradients interpolate in OKLCH (emitted as dense stops in SVG). Blending and opacity composite in sRGB, as browsers do, so SVG and PNG outputs agree. sRGB output by default.
Changing the blending space would shift every gradient and blend in every asset.
Shaders
Three tiers: built-in fields, field expressions compiled to SkSL, and a shader declaration holding raw SkSL. Shaders color shapes and never define geometry.
It fixes the paint model in the IR and the grammar.
Fields
Fields are first-class values usable as fill, mask, displacement, density or blend input.
It shapes the type system.
Scene lighting
Lights are declared once per scene with light, and layers opt in to respond.
Retrofitting lights would mean re-authoring every asset's shading.
Filter graph
fx lines may name their result with as: and later lines may reference it. A plain chain is the special case.
It fixes the syntax of every fx line.
Reserved keywords
shader, sksl, field, light, brush, warp, backdrop, profile, pose, joint, ease, t, frames, anim, key are reserved in v0.1 even where unimplemented.
Adding keywords later breaks files that used those words as names.
Deliberately left open, because each can be added later without breaking anything: poses, rigging helpers (joint limits, inverse kinematics), warping and limb deformation, backdrop effects, raster image inputs, wide-gamut output and animation semantics.
Language at a glance
An artscript file is a list of statements, one per line, shaped verb positional… key:value…, with indentation for children. Files use the .art extension. Below is a small project slice: a palette library, a component library, and one target file that uses both.
lib/palette/dusk.art (library: no @target)
palette dusk
  sky    ramp #1b1f3a #f2a65a 7     # 7 stops, dark to light
  skin   ramp oklch(.78 .07 55) 5 hue:20
  ink    #1a1423
  accent #ff5e5b
lib/face/eye.art (library)
use palette/dusk as p

def eye size:10 iris:p.sky.3 look:0,0
  circle size fill:#fff stroke:p.ink,1.5
    circle size*.55 at:look*size*.3 fill:iris    # clipped to the white
      circle size*.25 fill:p.ink
      circle size*.12 at:-.3,-.3 fill:#fff a:.9 # catchlight
art/chars/hero.art (entry file: declares its outputs)
@target chars/hero.svg 256x256
@target chars/hero.png 256x256 scale:1,2

use palette/dusk as p
use face/eye
use char/body
use outfit/tunic

canvas fill:p.sky.2

layer bg
  rect 100%x100% fill:grad(down p.sky.2 p.sky.5)
  noise worley scale:24 fill:p.sky.3 a:.15

layer hero at:50%,60%
  body build:slim skin:p.skin
    head:
      eye 9 at:-14,0 look:.4,0
      eye 9 at:14,0 look:.4,0
    torso:
      tunic color:p.accent
    .arm_r rot:-20             # override a part of the body by path
  fx shadow 0,6 blur:8 color:p.ink a:.3
Six conventions carry most of the language:
• Statements are verb args. The first one or two arguments are positional (circle 12, eye 9); the rest are key:value.
• Nesting a block under a shape clips the children to that shape. This is the single most common masking need in illustration (iris inside eye, pattern inside shirt) and it costs zero extra tokens. Nesting under layer or a component does not clip.
• name: followed by a block fills a slot that the component exposed (head:, torso:).
• A leading dot addresses a part of an instance (.arm_r rot:-20, .cape off), overriding it without editing the library.
• @ directives are file-level instructions to the build (@target); @name inside an expression refers to a named node's geometry (mask:@body).
• Comments start with #.
Values
Kind
Examples
Notes
Number
12, .5, -3
Canvas units (px at scale 1)
Percent
50%
Of the parent's bounds; resolves at evaluation
Angle
30deg, .25turn
Bare numbers in rot: are degrees
Vector
14,0, 256x256
x,y for points; WxH for sizes
Color
#ff5e5b, oklch(.78 .07 55), p.skin.3
Palette paths resolve to colors; OKLCH is the authoring space
Gradient
grad(down a b), radial(a b at:.3,.3)
Directions: down, right, or an angle
Field
worley(24), fbm(simplex(40)) * .6
A scalar value at every point; mapped to color with ->, to a shape with >
Shader
scales(6), holo()
A call to a shader declaration; usable as fill, stroke, blend or color filter
List / range
[plate, leather], 0..8
Used by each, repeat, pick
Reference
@body, @self
Geometry of a named node
Component
wizard_hat
Components are values and can be passed as arguments
Expressions support + - * /, comparison, if a then b else c, and a small pure function library (lerp, mix, clamp, shade, pick, rand). There are no unbounded loops or recursion, so every program terminates.
Grammar sketch
file      := directive* stmt*
directive := '@' ident arg* NEWLINE
stmt      := decl | control | check | node | slotfill | patch
decl      := 'use' path ('as' ident)?
           | 'palette' ident ('from' ident)? block
           | 'style' ident named*
           | 'def' ident named* block
           | 'let' ident '=' expr
           | 'shader' ident named* NEWLINE INDENT 'sksl' RAWSTRING DEDENT
           | 'light' ident named*
           | 'brush' ident named*
           | 'rule' ident named* block
check     := ('assert' | 'expect') expr named* STRING?
control   := 'if' expr block ('else' block)?
           | 'each' ident 'in' expr block
node      := ident expr* named* (NEWLINE block)?
slotfill  := ident ':' NEWLINE block
patch     := '.' path named* ('off')?
named     := ident ':' expr
block     := INDENT stmt+ DEDENT
Rendering primitives
The primitive set covers what an illustrator reaches for in Illustrator, Affinity or Figma, grouped into seven families. The MVP column marks what ships in v0.1; everything else is designed now so the syntax doesn't paint us into a corner.
Any node accepts the transform keys at: (position), rot:, scale:, skew:, pivot: and a: (opacity), plus name: to make it addressable by @name.
Transforms are relative to the parent node, and anchors and pivots move with the part that owns them. z: reorders a node among its siblings (z:behind, z:front, or a number), which is how a pose will later swing an arm behind the torso.
Rendering model and output profiles
Primitives mean what Skia draws. Each target picks an output profile, and the compiler checks every feature it uses against that profile before rendering anything.
Profile
Output
Features SVG can't express (shaders, lights, field fills, Skia-only filters)
vector
Pure SVG: every element stays editable vector
Compile error that names the feature and the node path
svg (default for .svg)
SVG, with some parts baked
Baked to embedded raster tiles, or to vector contours when thresholded
raster (default for .png)
PNG and later WebP
Rendered directly by Skia
Baked tiles render at the target's highest scale, so @2x assets never upscale. artscript check reports which nodes would bake under each profile, so authors can see the cost before choosing.
Color
Colors are authored in OKLCH and palettes, gradients interpolate in OKLCH, and blending and opacity composite in sRGB as browsers do, so an SVG and its PNG twin match. Outputs are sRGB by default. Wide-gamut output (Display P3) is a later opt-in per target, with out-of-gamut palette colors mapped by reducing chroma, never by clipping.
Geometry
Primitive
Example
Notes
MVP
Rectangle
rect 40x20 r:4
Per-corner radii with r:4,4,0,0
Yes
Circle, ellipse
circle 12, ellipse 20,12
Size is radius
Yes
Polygon, star
poly 6 r:20, star 5 r:20 inner:.5
Regular shapes; round: softens corners
Yes
Path
path "M0 0 c10-20 30-20 40 0z"
SVG path data, the escape hatch
Yes
Curve
curve 0,0 20,-15 40,0 closed
Smooth spline through points; models never place control points
Yes
Blob
blob 30 seed:4 wobble:.2
Organic closed shape for rocks, clouds, foliage
Yes
Boolean
subtract block, union, intersect, xor
Resolved to a single path at compile time
Yes
Mirror
mirror x block
Symmetric faces, armor, icons for half the tokens
Yes
Repeat, grid
repeat 8 rot:45, grid 4x3 gap:8
Radial and linear arrays
Yes
Offset, round
offset 4, round 3
Grow, shrink or soften any shape
Phase 2
Along
along @vine every:12
Place copies along a path, oriented to it
Phase 2
Scatter
scatter 40 in:@ground min:6
Poisson-disk placement with optional density field
Phase 2
Paint
Primitive
Example
Notes
MVP
Fill, stroke
fill:p.ink stroke:p.ink,2
Strokes default to round caps and joins
Yes
Gradients
grad(down a b), radial(a b at:.3,.3), conic(a b c)
Interpolated in OKLCH to avoid muddy midpoints
Yes
Tone helpers
shade(c, -2), lit(c, 1)
Step along the color's palette ramp, shifting hue (warm light, cool shadow)
Yes
Blend
blend:multiply, blend:holo()
All Skia blend modes, or a custom blender shader
Yes (built-in modes)
Field fill
fill:worley(24) -> p.stone
A field mapped through a palette ramp
Yes
Shader fill
fill:scales(6)
Any shader declaration as paint
Phase 2
Variable-width stroke
stroke:p.ink,1..3
Tapered ink lines; outlined to a filled path at compile time
Phase 2
Brush
brush ink stamp:@dab jitter:.5 taper then stroke:ink
Named stroke styles built from Skia path effects: stamp, jitter, dash, trim, corner
Phase 2
Pattern
pattern hatch 45deg gap:4
Hatch, crosshatch, dots, stripes, or any component as a tile
Phase 2
Mesh gradient
mesh 3x3 [colors]
Soft volumetric shading via Skia's patch drawing
Phase 2
Layers and masks
Primitive
Example
Notes
MVP
Layer
layer hair blend:screen a:.8
Named, isolated group; becomes <g> in SVG
Yes
Clip by nesting
shape with an indented block
Children are clipped to the parent shape
Yes
Mask
mask:@body, mask: block, mask lum
Alpha or luminance masks from any node, layer or field
Yes
Inside / outside
inside @torso block
Draw only within (or outside) another node's silhouette; the core of composable clothing
Yes
Feather
feather:6
Soft mask edge
Yes
Soft clip
clip:simplex(30) > .5 feather:3
Clip directly by a field, without a separate mask layer
Phase 2
Knockout
knockout @hand
Punch a referenced shape out of the current layer
Phase 2
Backdrop
layer glass backdrop:blur(12)
Filter what is behind the layer (frosted-glass UI); raster profile only
Phase 3
Filters (fx)
fx lines apply to their parent node and render with Skia's image filters. Under the svg profile, effects with an exact SVG equivalent (blur, shadow, color matrix) are emitted as SVG filters; the rest bake. Effects form a graph: a line can name its result with as:, and a later line can use it. A plain list of lines is the special case where each feeds the next.
layer water
  rect 100%x40% fill:grad(down p.sea.3 p.sea.5)
  fx blur 6 of:@sky as:soft          # blur the sky layer
  fx displace 4 by:simplex(30) of:soft as:ripple
  fx merge ripple self a:.4,1         # reflection under the water
Effect
Example
MVP
Blur
fx blur 4
Yes
Drop shadow
fx shadow 0,6 blur:8 color:p.ink a:.3
Yes
Inner shadow
fx inner 0,-3 blur:4 color:p.ink a:.4
Yes
Glow
fx glow 8 color:p.accent
Yes
Adjust
fx adjust hue:10 sat:-.2 light:.05
Yes
Outline
fx outline 2 color:p.ink
Phase 2
Grain
fx grain .08
Phase 2
Displace
fx displace 6 by:noise(simplex 30)
Phase 2
Bevel (responds to scene lights)
fx bevel 3 lit:sun
Phase 2
Posterize, halftone
fx halftone 6 angle:15
Phase 3
Procedural texturing
Noise is a value, not just a filter, so the same field can fill, mask, displace or drive density.
• Built-in fields: simplex, perlin, worley, value, with fbm and ridged modifiers, all taking scale, oct: and seed:.
• Field expressions: fields combine with arithmetic and functions (fbm(worley(24)) * .6 + simplex(80) * .4). The compiler turns each expression into one SkSL shader, so the model composes textures without writing shader code.
• As fill: fill:worley(24) -> p.stone maps the field through a palette ramp.
• As mask or clip: mask:simplex(40) > .5 thresholds the field into a shape (moss, rust, cloud edges).
• As geometry input: jitter:1.5 on any shape gives a hand-drawn wobble; scatter can take a field as density; hatch and stipple generate strokes from a tone field.
• Output: under raster, Skia renders fields directly. Under svg, continuous fills bake to raster tiles and thresholded fields to vector contours. Under vector, only thresholded fields are allowed.
Shaders
When built-in fields aren't enough, a library declares a shader with raw SkSL. It is written once and reused like a component; most target files never contain shader code.
# lib/shader/scales.art
shader scales size:8 base:p.green.3 rim:p.green.5
  sksl """
  half4 main(float2 p) {
    float2 c = fract(p / size) - .5;
    float d = length(c + float2(0, .5));
    return mix(base, rim, smoothstep(.35, .5, d));
  }
  """
blob 40 fill:scales(6)          # as paint
layer foil blend:holo()         # as a custom blender
fx grade warm:.2                # a shader with a color-filter signature, as an effect
The rules below are fixed in v0.1, even though raw SkSL ships in Phase 2:
1. Role by signature. main(float2 p) makes a shader (paint, fill or mask). main(half4 c) makes a color filter, usable in fx. main(half4 src, half4 dst) makes a blender, usable in blend:.
2. Parameters become uniforms. Numbers map to float, vectors to float2, colors to half4. A parameter can also take a field, a shader or a node reference, which arrives as a child shader, so shaders compose.
3. Units, not pixels. p is in the node's local canvas units, so a pattern looks the same at @1x and @2x. Built-in uniforms: size (node bounds), seed, scale (output scale), and t, reserved for animation.
4. Colors arrive in the working space, converted from OKLCH, so a shader's mix matches neighboring gradients.
5. Shaders color shapes; they never define them. Shapes drawn inside shaders (signed-distance fields) would break booleans, @ references and path overrides, so artscript has no way to turn a shader back into geometry except thresholding a field.
6. Bounded and checked. SkSL forbids I/O and unbounded loops. The compiler type-checks each shader at artscript check time and reports SkSL errors at the line inside the .art file.
Lighting
Lights are declared once per scene and layers opt in, so every asset in a set shares one light direction and one highlight and shadow color.
light sun dir:-1,-1.5 color:p.warm.5 ambient:p.cool.2

layer hero lit:sun
  body
  fx bevel 3                 # shading follows sun
  fx rim 2 color:p.warm.5    # rim light on the side facing it
Lights feed Skia's lighting filters (distant, point and spot lights over normals derived from shape alpha) and are passed to shaders as uniforms. Tone helpers like lit() and shade() can also read the active light, so flat-color art gets consistent value structure without filters. Under svg and vector profiles, light-driven effects bake or fail like any other raster-only feature.
Typography (Phase 2)
text "Play" font:display size:24 track:.05 uses fonts declared in the manifest and is outlined to paths by default, so output never depends on installed fonts.
Seeds
Every random function takes seed:. When omitted, the seed is a hash of the file path and the node's path, so outputs are stable and editing one node never reshuffles another's texture.
Reuse and composition
artscript provides seven general mechanisms and no domain models. Characters, outfits, faces, tile sets and icon families are all ordinary project libraries built from them, so each project can invent the composition scheme it needs.
The mechanisms
1. Modules. use path imports a library file, resolved against the lib roots in the manifest; use path as x aliases it; use path { a, b } imports selected names. Everything top-level is exported except names starting with _.
2. Palettes are values. A palette holds named colors and ramps. palette night from dusk inherits and overrides. Because palettes are values, a component can take one as a parameter (pal:dusk), which is how faction colors, day/night themes and skin tones swap without touching geometry.
3. Styles. style ink stroke:p.ink,2 join:round names a bundle of attributes; style:ink on a node applies it to the node and its descendants. Styles keep line weight and finish consistent across an art set.
4. Components (def). Named, parameterized drawings with defaults: def eye size:10 iris:p.sky.3. Parameter types are inferred from defaults. The first parameter is positional at the call site.
5. Anchors and slots. anchor neck 0,-4 exposes a named point; slot head at:0,-30 exposes an insertion point in the component's z-order. Callers fill slots with head: blocks, and slots may hold default content.
6. Host references. Inside slot content, @name resolves first against the host component's named nodes. A tunic that draws inside @torso works on any body that names a part torso. A component can declare what it expects with needs:@torso, which turns a missing part into a clear error instead of a silent misdraw.
7. Path overrides. A leading dot patches a part of an instance from outside: .arm_r rot:-20, .cape off, .head.eye_l iris:p.accent. This covers one-off variations without forking a library. Overrides apply in source order and the last value wins, and they can reach transforms, visibility and draw order (.arm_r z:behind).
Components are first-class values, so higher-order composition comes free: a component can take another as a parameter and place it in a slot.
Worked example: composable characters
lib/char/body.art
use palette/base as p

def body build:avg skin:p.skin
  let w = if build == slim then 34 else 42
  curve -w/2,0 w/2,0 w*.4,70 -w*.4,70 closed fill:skin name:torso
  slot torso                      # outfits draw right above the torso
  rect 8x40 at:w/2,6 r:4 fill:shade(skin,-1) name:arm_r pivot:0,-18
  anchor neck 0,-4
  circle 26 at:0,-30 fill:skin name:head
  slot head at:0,-30              # faces, hair, hats
lib/outfit/tunic.art
use palette/base as p

def tunic color:p.accent trim:p.ink needs:@torso
  inside @torso
    rect 100%x60% at:0,30% fill:color
    rect 100%x3 at:0,58% fill:trim
  fx inner 0,-2 blur:3 color:shade(color,-2) a:.5
lib/char/villager.art (a higher-order component)
use char/body
use face/basic
use palette/base as p

def villager outfit:tunic hat:none skin:p.skin
  body skin:skin
    torso:
      outfit
    head:
      basic_face
      hat
A target can then build a crowd: villager outfit:apron hat:straw_hat for one figure, or pick([tunic, apron], seed:i) inside an each for variety. None of these names (body, torso, outfit) mean anything to the compiler; another project might rig vehicles, buildings or UI chrome the same way.
Which mechanism for which need
Need
Mechanism
Color palettes and themes
palette with ramps; pass palettes as parameters; palette x from y for variants
Composable clothing and armor
Slots on the body, inside @part in the garment, needs: for contracts
Facial features
Components placed at anchors; mirror x for symmetry; parameters for expression
Sprite variations (color, gear, pose)
@target … each matrices (see Targets) plus path overrides
Consistent line work across a set
style bundles applied at layer level
Tile sets and icon families
Shared components and styles, one entry file per sheet
Crowds and foliage
each, scatter, pick with seeds
Visual checks
Authors write spatial checks inline, next to the drawing they protect, with assert (a failure) and expect (a warning). Checks measure the evaluated scene, never change it, and never stop outputs from being written: a failed check makes the command exit non-zero while every asset still lands on disk for inspection.
Models reliably get structure right and measurements wrong: eyes a few units off level, a head too large for the body, icons that drift off a shared baseline. A check turns the intent behind a drawing ("eyes are level", "head is about a seventh of the height") into something the build verifies on every run and every variant.
Syntax
def face size:26
  circle size name:head
  eye 4 at:-8,-2 name:eye_l
  eye 4 at:8,-2 name:eye_r
  assert level(@eye_l, @eye_r) tol:.5
  assert symmetric(@eye_l, @eye_r, about:@head.cx) tol:.5
  expect gap(@eye_l, @eye_r) in .8..1.2 * @eye_l.w "eyes about one eye-width apart"
# art/chars/hero.art
layer hero at:50%,60%
  body build:slim
  assert @hero.head.h / @hero.h in 1/8..1/6 "stylized heroic proportions"
  assert inside(@hero, canvas, margin:8)
  expect contrast(@hero, @bg) >= 3
• Both fail the build by default. A failed assert is an error; a failed expect is a warning, which --allow-warnings lets pass. Both take an optional message. Without one, the compiler writes a message from the expression and the measured values.
• Scope follows placement. A check inside a def runs for every instance in every target, so a library proves its own invariants once. A check in a target file covers the composed scene.
• Checks run per output. Each each variant is checked separately, so a check like @hero.feet.bottom ~= 120 keeps every sprite in a matrix on one baseline.
• Comparisons with tolerance: ~= with tol:, ranges with in a..b, and the usual < <= > >=.
• Wildcards: all(@row.*, aligned:bottom) checks a whole group.
Measurements
Geometric measurements read exact geometry from the scene IR, after booleans and overrides resolve. They are cheap and need no rendering. Visual measurements read the rendered pixels.
Kind
Measurements
Notes
Bounds
@a.x .y .w .h .cx .cy .left .right .top .bottom .area
Visual bounds, including stroke width; @a.geom.* excludes strokes
Position
inside(@a, @b, margin:), overlaps(@a, @b), touches(@a, @b), gap(@a, @b), dist(@a, @b)
canvas and safe (the safe area) are built-in references
Alignment
aligned(@a, @b, edge:top), level(@a, @b), centered(@a, in:@b), on_grid(@a, 4)
edge: takes top, bottom, left, right, cx or cy; level compares vertical centers
Proportion
Arithmetic on bounds, ratio(@a, @b), aspect(@a)
Ranges express style targets, such as heads per body height
Symmetry
symmetric(@a, @b, about:), mirror_match(@a, axis:)
mirror_match compares a shape with its own reflection
Consistency
same_size(@icons.*), stroke_widths(@a)
For sets of icons, tiles and frames
Visual (rendered)
contrast(@a, @b), visible(@a), occluded(@a, by:), value_range(@a)
contrast uses the WCAG luminance ratio; visible is the fraction left unoccluded
Reusable rules
A rule declaration bundles checks under a name, lives in libraries and is applied like a function:
# lib/rules/icons.art
rule icon_grid subject:@self size:64
  assert inside(subject, canvas, margin:size * .0625)
  assert on_grid(subject, 2)
  expect stroke_widths(subject) == [2]

# art/ui/icons.art
assert icon_grid(@icon)
The manifest can apply rules to every target matching a glob ([checks] "ui/*" = ["rules/icons.icon_grid"]), so house standards hold even when a model forgets to write them.
Results and exit codes
• Every output is written, even when checks fail, because the failing asset is the evidence.
• Exit codes: 0 when everything passes; 1 when a compile error means a target couldn't be produced; 2 when every target was produced but at least one assert failed; 3 when the only failures are warnings (failed expect checks or lints set to warn). Warnings fail by default; --allow-warnings lets them exit 0 for local iteration, and info-level lints never fail.
• The manifest records results. art-manifest.json marks each output pass, warn or fail with the failing checks, so a game's asset pipeline can block failed art.
• Diagnostics name the fix. A failure prints measured and expected values, the node paths involved and, when one node's position or size explains the gap, the nudge that would satisfy it:
error[C0102] lib/face/face.art:6:3  hero.body.head.face
  assert level(@eye_l, @eye_r) tol:.5
  eye_l.cy = -2.0   eye_r.cy = 1.5   difference 3.5 > tol .5
  fix: set .eye_r at:8,-2
• Overlays show the failure. For each failing output, the build writes an annotated PNG to .artscript/checks/ that draws the measured boxes, centers and gaps over the art in red. A multimodal model can look at the overlay and the asset side by side.
Checks verify; they never move anything. Layout helpers (aligning, distributing, anchoring) belong in drawing code, so that a check failing always means a real mistake rather than a fight with a solver.
Targets
A file becomes an entry file by declaring at least one @target; a file with none is a library. That declaration is the only thing the build looks at to decide what to render, so every output can be traced to exactly one line in one source file.
Syntax
@target <out-path> <size> [key:value…]
• out-path is relative to the manifest's out directory. The extension sets the format: .svg or .png in v0.1; .webp and sheet formats later.
• size is the canvas in units, e.g. 256x256.
• scale:1,2,3 writes one file per scale (hero.png, hero@2x.png, hero@3x.png). It applies to raster formats only.
• bg:none|<color> overrides the canvas background for this target. as:<label> names the declaration so the file body can branch on it. profile:vector|svg|raster picks the output profile (see Rendering model and output profiles); by default .svg targets use svg and .png targets use raster.
• each name:[values] fans one declaration out into a matrix. Each variable is available in the file as a constant and is interpolated into the path with {name}.
@target icons/{name}.svg 64x64 as:icons each name:[sword, shield, potion]
@target chars/knight-{armor}-{team}.png 128x128 scale:1,2 as:knights each armor:[plate, leather] team:[red, blue]

use icons/set
use char/knight
use palette/teams as t

if target == icons
  icon name                         # dispatch on the matrix value
else
  knight armor:armor pal:t.{team}
The second line produces 8 files (2 armors × 2 teams × 2 scales), all traced to that one declaration. When a file holds several @target lines, the built-in constant target holds the as: label of the declaration being rendered, so one scene can serve several outputs.
Rules
1. A file with one or more @target lines is an entry file. A file with none is a library. There is no other classification mechanism.
2. @target lines come first, before any other statement, so a reader or a model sees a file's outputs at a glance.
3. Entry files cannot be imported. use of an entry file is error E0201. Shared code moves to a library. This keeps the dependency graph one-directional: entries depend on libraries, never the reverse.
4. Each output path has exactly one source. Two declarations that produce the same path, after matrix expansion, is error E0202, naming both lines.
5. Library roots are enforced. Directories listed under lib in the manifest must not contain @target (error E0203). Entry files can live anywhere else; the recommended home is art/.
6. The build owns its outputs. The build records every file it writes in art-manifest.json. On rebuild it deletes previously generated files whose declaration is gone, and never touches files it did not create.
7. Libraries are inert. A library is evaluated only when an entry imports it, and its top-level statements may only be declarations (use, palette, style, def, let). A stray drawing statement at a library's top level is error E0204.
Project layout and CLI
artscript build, a single self-contained Rust binary run anywhere inside a project, renders every @target in the project and nothing else. The project root is the nearest ancestor directory holding artscript.toml.
Layout
skyforge/
  artscript.toml          # manifest: marks the project root
  lib/                    # library root: no @target allowed
    palette/dusk.art
    char/body.art
    outfit/tunic.art
    face/eye.art
  art/                    # entry files (convention, not a rule)
    chars/hero.art
    ui/icons.art
  fonts/Fraunces.ttf
  assets/gen/             # build output (from manifest `out`)
    art-manifest.json
  .artscript/cache/       # incremental cache, gitignored
Manifest
[project]
name = "skyforge"
out  = "assets/gen"
lib  = ["lib"]           # import roots; @target forbidden here
std  = "0.1"             # standard library version

[defaults]
palette = "palette/dusk"  # available as `p` in every file
scale   = [1]
bg      = "none"
profile = "auto"          # vector | svg | raster | auto (by extension)

[fonts]
display = "fonts/Fraunces.ttf"

[lint]
raw-color = "warn"
Commands
Command
What it does
artscript build
Discovers every .art file under the root (respecting .gitignore), renders all targets in parallel, writes outputs and art-manifest.json. Exits 0 when everything passes, 1 on a compile error, 2 when outputs were written but an assert failed, 3 when only warnings failed. --allow-warnings turns 3 into 0.
artscript build --watch
Rebuilds affected targets on save.
artscript build --only 'chars/*'
Renders targets whose output path matches the glob.
artscript check
Parses, type-checks and lints without writing outputs; --json for machine-readable diagnostics.
artscript fmt
Rewrites files in the one canonical format.
artscript explain <file>
Prints the evaluated node tree with paths, sizes and resolved colors, for debugging and for LLM edits.
artscript preview <glob>
Renders matching targets into one PNG contact sheet, optionally labeled with node paths (Phase 2).
artscript init
Creates a manifest, a starter palette and an example entry file.
Build behavior
• Incremental. Each target's cache key hashes its entry file, every transitively imported library, the manifest, fonts and the compiler version. Unchanged targets are skipped.
• Deterministic. Same inputs, byte-identical outputs: fixed float formatting, sorted attributes, seeded randomness, no timestamps in files.
• Traceable. art-manifest.json maps each output to its source file and @target line, its matrix values and its content hash. Game engines and asset pipelines can read it directly.
• Fail per target. An error in one entry file fails that file's targets and the overall exit code, but the build still renders everything else and reports all errors together. Failed checks never stop outputs from being written (see Visual checks).
Compiler and renderer architecture
The compiler is one static binary that runs a linear pipeline, with a single scene IR as the format every backend reads.
The first four stages run once per file; from Evaluate on, the work runs once per target and in parallel. Because backends only ever see the resolved scene IR, new outputs (WebP, sprite atlases, Lottie) are new backends rather than language changes.
• Scene IR is a typed tree of layers, shapes, paints, masks, lights and effects with every value resolved to absolute numbers. Its paint and effect model mirrors Skia's (shaders, color filters, blenders, image-filter graphs), so the Skia backend is a direct translation. It is also what artscript explain prints.
• Skia is the driver, through the skia-safe Rust bindings. It provides rasterization, PathOps for booleans, stroking and path effects for brushes, image filters, SkSL runtime effects for fields and shaders, and text shaping.
• The SVG writer is artscript's own. It lowers the scene IR to SVG elements where an exact equivalent exists, and asks Skia to bake the rest according to the target's profile. Skia's experimental SVG canvas is not used, because it can't keep artscript's layer names and node paths.
• Committed outputs render on Skia's CPU path at a Skia version pinned by each artscript release. A Skia upgrade that changes pixels ships as a minor version with a changelog entry, and the snapshot suite catches unintended changes. GPU rendering is used only for --watch and preview.
• Implementation language: Rust. The parser, compiler, renderer and CLI are one Rust program, shipped as a single self-contained executable per platform with Skia statically linked: no runtime, interpreter or shared libraries to install, so CI, game build machines and model sandboxes can each run it after one download. Releases cover macOS (arm64, x64), Linux (x64, arm64) and Windows (x64). The trade-off is a larger binary and harder cross-compilation because of Skia; prebuilt skia-safe binaries cover the main desktop platforms.
• Playground: CanvasKit (Skia compiled to WASM) runs the same SkSL in the browser, so a web editor renders exactly what the CLI renders.
Visual checks run in two places. Geometric checks run right after the scene IR passes, before any backend, so they cost almost nothing. Visual checks run after Skia renders each target's highest scale. Both feed art-manifest.json and the overlay renderer, and neither can change an output.
LLM ergonomics
The primary author is a model, so the toolchain is designed around a write, check, look, patch loop: the model writes a file, reads structured diagnostics, looks at a render, and patches specific nodes by path. Inline visual checks let the model state its intent up front, so the build catches proportion and alignment drift the model can't see in its own code.
The language card
The whole language reference ships as one compact document (target: under 2,500 tokens) meant to sit in a system prompt, plus the project's own library signatures generated by artscript explain --signatures. A model should be able to write correct artscript from the card alone, with no other documentation.
Diagnostics written for models
Every diagnostic carries a code, a location, the node path, a one-line cause and a concrete fix. --json emits the same fields for agents.
error[E0311] art/chars/hero.art:18:7  hero.body.torso.tunic
  tunic needs @torso, but the host `goblin` has no part named torso
  parts available: chest, belly, arm_l, arm_r
  fix: rename goblin's `chest` to `torso`, or pass `inside:@chest` to tunic
Taste lints
Lints encode the habits of a good art director. Each can be set to off, info, warn or error in the manifest. Warnings fail the build like errors unless --allow-warnings is passed; info never fails.
Lint
Flags
Default
raw-color
Literal colors outside a palette block
warn
palette-sprawl
A target using more distinct hues than the palette defines
warn
value-contrast
Figure and ground too close in lightness when rendered in grayscale
warn
off-canvas
Content clipped by the canvas edge or entirely outside it
warn
stroke-drift
Stroke widths in one target that don't come from a shared style
info
near-tangent
Edges that almost touch (under 1.5 units), a classic composition flaw
info (Phase 2)
unused
Unused imports, parameters, slots or let bindings
warn
Visual feedback
artscript preview renders a contact sheet a multimodal model can inspect. With --paths, it overlays node paths on the image, so a critique like "the left eye sits too high" maps straight to .head.eye_l at:-14,-2.
Measuring token efficiency
The token-efficiency goal is tested, not asserted. A benchmark corpus of reference images (icons, characters, tiles, UI panels, backgrounds) is authored in both artscript and hand-optimized SVG. CI tracks the token ratio for each candidate keyword and syntax choice, and a keyword survives only if it tokenizes as one token in the tokenizers we target.
MVP scope and roadmap
v0.1 proves the core loop (write, build, look, patch) with enough primitives to rebuild 20 reference pieces in at least 3x fewer tokens than SVG. Later phases close the remaining quality gap and then reach into game and app pipelines.
The phases are sequenced, not dated; each gate is a measurable bar rather than a calendar milestone.
What v0.1 includes
Area
In v0.1
Deferred
Language
Statements, indentation, expressions, let, if, each, use; every reserved keyword
User functions (fn), animation semantics
Reuse
palette with ramps and inheritance, style, def, anchors, slots, host references, needs:, path overrides
Git dependencies between projects
Targets
@target with size, scale, bg, as:, profile:, each matrices; all seven rules
Sprite sheets, atlases, 9-slice
Visual checks
assert, expect, rule; geometric measurements; exit codes; results in the manifest; diagnostics with suggested fixes
Rendered measurements (contrast, visibility), overlays, rules applied from the manifest
Rendering model
Skia driver, three output profiles, color pipeline, pinned CPU rendering
Wide-gamut (P3) output
Geometry
rect, circle, ellipse, poly, star, path, curve, blob, booleans via PathOps, mirror, repeat, grid
offset, round, along, scatter, warp
Paint
Fill, stroke, linear, radial and conic gradients, tone helpers, blend modes, field fills
Shader fills, brushes, variable-width strokes, patterns, mesh gradients
Layers and masks
Layers, clip by nesting, masks, inside, feather
Soft clip, knockout, backdrop
Filters
blur, shadow, inner, glow, adjust; as: graph wiring
outline, grain, displace, bevel, rim, halftone
Fields
Built-in fields and field expressions compiled to SkSL, as fill and mask; jitter; baking per profile
hatch, stipple, field-driven scatter
Shaders and lights
Grammar and rules fixed; shader and light parse and report "not yet implemented"
Raw SkSL shaders, scene lights, light-aware tone helpers
Output
SVG, PNG at multiple scales, art-manifest.json
WebP, text, Lottie
Tooling
build, build --only, check, fmt, explain, init; lints raw-color, off-canvas, unused
--watch, preview, snapshot tests, LSP, remaining lints
Open questions and risks
The biggest risk is that models write syntactically valid artscript that still looks mediocre; the taste defaults, lints and preview loop exist to counter it, and the benchmark has to measure visual quality, not just tokens.
Question
Options
Current leaning
Indentation vs braces
Significant indentation; { } blocks
Indentation: fewer tokens, and fmt removes ambiguity. Risk: models mis-indent in long files, so the parser should report the likely intended parent
Expression power
Pure expressions only; add user functions (fn)
Start with let and def only; add fn if libraries need computed geometry
Raw hex colors
Allow anywhere; palette-only
Allow, with the raw-color lint on by default
Raster image inputs
Purely generative; allow image("tex/paper.png") fills and 9-slice frames
Allow later, from project files only, so builds stay hermetic
Wide-gamut output
sRGB only; opt-in Display P3 per target
Opt-in P3 once the color pipeline is proven in sRGB
Skia upgrades
Pin per release; track Skia milestones closely
Pin per release and upgrade on a fixed cadence, gated by the snapshot suite
Binary size and platforms
Accept Skia's size; ship a slim build without GPU
Accept for v0.1; revisit if CI and container users object
LLM-written SkSL
Allow in any file; restrict to libraries
Libraries only at first, so shaders get reviewed once and reused
Pose and rigging
Anchors and rotation only; a skeleton primitive
Settled for v0.1 by four rules (hierarchical transforms, override order, pure evaluation, node paths as identity; see Foundational decisions). Poses, joint limits and inverse kinematics are later additions
Sharing libraries across projects
Git dependencies; a package registry
Git dependencies in the manifest first; a registry only with demand
Animation
Out of scope; keyframes compiled to Lottie or CSS; frame strips
Keywords reserved now (t, frames, anim, key); the four pose-readiness rules keep it additive; design in Phase 3
File extension
.art, .arts, .ascr
.art is short but collides with some legacy image formats; confirm before v0.1
