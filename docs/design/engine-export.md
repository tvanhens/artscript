# Engine export

This note decides how ramose animation exports into a game engine. Godot 4 is the reference engine. ramose has no engine export yet, so nothing here is in the author card. #290 moves the route table below into the card. #299 builds the retained IR that an exporter reads.

The goal is that a game team can import a ramose character and play it in the engine. At each frame time, the engine must show what ramose draws, apart from the engine's own antialiasing and texture filtering. Where the engine cannot draw a node or carry its change as a track, ramose bakes frames, and the export says which nodes it baked and why. No change is ever dropped.

## One entry exports one scene

An export is one output of a target, like a PNG, with one `scale:` and one cell of the `each` matrix. All of an entry's clips at that output export into one scene with one animation player. The rest pose, the scene with no play applied, is the engine's reset state. In Godot, that is the `RESET` animation of an `AnimationPlayer`. Every clip must give the same node tree as the rest pose, because one scene has one tree. If `if clip == walk` draws a part that `idle` does not, the export stops with an error at the first node that differs.

Each `clip:` exports as one engine animation with the clip's name, the target's `fps:`, and the clip's length. Its tracks hold every play that the clip reaches, so a clip that plays `idle` and `blink` together is one engine animation. The animation loops when any play that the clip reaches loops. ramose already uses that rule when it measures a clip. Engine state machines and blends between clips are #294.

The exporter samples each clip at every frame time in `[0,dur)`, the same times a GIF draws. A property that differs from the rest pose at any sample gets a track in that clip, with one key per frame. A property that never differs from the rest pose is a static value on the node. The export is a build output, so the same source, manifest, and backend give the same bytes.

Keys at every frame are the only way to match ramose at each frame time. ramose blends colors in OKLab, and Godot blends a color track in sRGB. A Godot color track from red to blue gives `(0.5, 0, 0.5)` at its midpoint. ramose eases also have no exact engine equivalent. A key interpolates linearly to the next key, unless ramose steps between those two frames. A step is a `hold` ease, a word or a boolean that changes, or a play that starts or ends. A key before a step holds its value until the next key, so the engine never shows a value that ramose does not draw. Godot sets interpolation per track, so a hold is a key with transition 0. A format with no per-key hold gets a second key with the held value just before the next frame. Visibility and flipbook frame tracks are always discrete. A later change can fit engine curves to the samples and drop keys when the error stays under one 8-bit step. Compiler size and export time are not constraints.

The exporter routes each change by the resolved value that changes, not by the patch that wrote it. `.mouth open:key(0 1 0)` changes the points of the curves inside the `mouth` def, so it is a vertex track on those curves. A `style:` that steps between two styles routes as the keys it changes. A def parameter, a `let`, and an anchor reference route the same way.

If a node exists at one sample and not at another, or changes its parent, verb, or draw order, the export stops with an error at that node. That is a structure break (#288). The exporter reports it whether or not `check` reports it yet (#289).

## Track kinds do not depend on the format

The route table maps each property to a track kind. A track kind is an engine concept, not a file format. Each export format declares the track kinds it carries. If a format lacks a track kind, the node bakes to a flipbook with the reason `format`. The route table stays the same for every format.

| track kind | what it carries | Godot 4 reference |
|---|---|---|
| transform | a node's local position, rotation, and scale | `Node2D` `position`, `rotation`, `scale` |
| skew | a node's local skew | `Node2D` `skew` |
| opacity | the alpha of one engine item | `modulate:a` |
| group opacity | the alpha of a composite of several engine items | `self_modulate:a` on a `CanvasGroup` |
| visibility | whether a node draws | `visible` |
| color | one flat paint color | `Polygon2D` `color` |
| vertex | the points of a mesh whose triangles stay the same | `Polygon2D` `polygon`, with fixed `polygons` |
| bone | a bone's transform, and the weighted parts that follow it | `Skeleton2D` with `Bone2D`, and a weighted `Polygon2D` |
| material | a number or color in a material | `ShaderMaterial` `shader_parameter/<name>` |
| flipbook | baked frames of a subtree | `Sprite2D` with `hframes` and `vframes`, and a `frame` track |

Godot 4.6.1 runs these references, measured with the Compatibility renderer (OpenGL). A headless script animated `polygon`, `skew`, a shader parameter, `CanvasGroup` `self_modulate`, `color`, and `frame`, and each track gave the interpolated value at its midpoint. A `polygon` track interpolates when both keys hold the same number of points. A rendered script showed more. A `CanvasGroup` at alpha 0.5 fades two overlapping black squares as one, with the overlap as light as the rest. A `Polygon2D` whose `polygons` the script wrote draws a ring with an empty hole. `clip_children` is not usable for ramose clips. A clip nested in a clip drew the wrong region, and a clip inside a `CanvasGroup` let its child spill out, as the Godot class reference warns. An `int` track such as `frame` interpolates unless its update mode is discrete. Bones and visibility are not measured yet.

## What the engine draws as a node

A node exports as engine nodes only when the engine can draw it exactly. Every other node bakes. This list is the whole set of native features, so a feature that is not on it bakes by default:

- A container: `group`, `layer`, an instance, `mirror`, `repeat`, or `grid`. Each copy of a `mirror`, `repeat`, or `grid` is its own node.
- A shape with a flat `fill:`, a flat `stroke:`, `fill:none`, a gradient fill, `dash:`, `cap:`, `join:`, `taper:`, a width list, or `jitter:`. A static boolean is a shape too. The exporter flattens the shape, merges any part that overlaps itself, and writes its own triangles, so holes and several contours draw correctly. A gradient fill is a `Polygon2D` with a ramose gradient shader.
- A stroke, as its own `Polygon2D` of the stroke's outline, drawn above the fill.
- A child clipped by nesting. The exporter intersects the child's mesh with the shapes of every ancestor that clips it, and writes the result. A child that stays inside its clip on every frame keeps its own mesh. A `fill:none` or a translucent parent still clips to its whole shape, as ramose does. The engine's own clip is not used, because Godot cannot nest it or put it inside a `CanvasGroup`.
- The node keys `at:`, `rot:`, `scale:`, `skew:`, `pivot:`, `a:`, `order:`, and `name:`, and `off`.

A node bakes when it uses anything else. That includes `brush:`, `stamp:`, a field fill, `blur:`, `warp:`, a mask, `feather:`, an `fx` line, a `blend:` other than the default, and a node under `inside` or `outside`. A baked node with no change over any clip is a still, a flipbook of one frame.

The exporter flattens each contour at a fixed point count for the whole entry. The count keeps every frame's polyline within 1/8 of an output pixel of the shape ramose draws, at the largest scale that any clip of the entry reaches. A `curve` gets a fixed count per segment, an `edge` resamples by arc length, and a round cap or join gets a fixed count per arc. The count does not depend on the frame, so a moving or growing shape keeps its triangles.

## The route table

Each row is a resolved property that can change during a clip. The route applies to a native node. A change inside a node that already bakes makes that bake a flipbook.

| property | route | notes |
|---|---|---|
| `at:`, `rot:`, `scale:`, `pivot:` | transform | The exporter decomposes the node's whole local matrix at each frame into position, rotation, two scales, and one skew. Any 2D affine matrix decomposes exactly that way, so `pivot:`, a percent `at:`, and an anchor `at:` need no extra nodes. The x scale stays positive, and a mirror sets the y scale negative, so the decomposition picks one branch on every frame. Rotation keys unwrap, so the engine never turns the long way. |
| `skew:` | skew | A two-axis `skew:` decomposes into rotation, scale, and one skew, like the row above. |
| `a:` on a shape that exports as one `Polygon2D` with no stroke and no descendants | opacity | |
| `a:` on any other node | group opacity | ramose fades a node with its fill, its stroke, and its children as one composite. A shape with a fill and a stroke is two engine items, and a shape with children has engine children. Godot `modulate` fades each item on its own, so these fade as a `CanvasGroup`, as a container does. |
| `off` in an anim | visibility | Today `off` hides the node for the whole anim. A keyed spelling that hides it for part of a clip is #290's to choose. |
| `fill:` or `stroke:` with a flat color, or `fill:none` | color | `none` is a color with alpha 0, so a change between `none` and a color is a color track. |
| a gradient's stop colors, stop positions, or end points | material | Each changing number or color is a shader parameter. A change in the number of stops bakes, with the reason `topology`. |
| a shape's size numbers: `rect` size and `r:`, `circle` and `ellipse` radii, `poly` and `star` `r:`, `star` `inner:`, `round:`, `blob` size and `wobble:`, `curve` points, `tension:`, `jitter:` | vertex | |
| `stroke:` width, a width list, `taper:` | vertex | |
| `edge` `from:` and `to:`, a `copy` or an `edge` of a vertex-animated shape | vertex | |
| the operands of a static boolean | vertex, or flipbook with the reason `topology` | A vertex track holds while the result keeps its contours and triangles. |
| a clipped child's position, or its clip's shape | transform, vertex, or flipbook with the reason `topology` | A child that stays inside its clip keeps its transform track. Otherwise the clipped mesh is a vertex track while its triangles stay the same. |
| a bone's `at:`, `rot:`, `scale:` | bone | Bones are #193. A part that follows one bone with no weights is a transform child of that bone. |
| `poly` and `star` point count, `blob` `points:` | flipbook, `topology` | The contour changes its point count. |
| a fill or stroke that changes kind, such as a flat color to a gradient | flipbook, `topology` | |
| `order:` | none | Draw order is part of the structure (#288), so a change is a structure break. |
| any other change | flipbook, `raster` | A key added later starts in this row until the table gives it a route. |

A vertex route still needs the same mesh on every frame. The exporter checks each sample. If a contour count, a closed or open flag, a point count, or a triangle list differs from the rest pose, the node bakes with the reason `topology`. An outline that crosses itself on one frame merges into a different mesh there, so the same check catches it. The report names the first frame that differs.

## A flipbook bakes the smallest subtree that holds the change

A bake covers a node and its descendants. The baked node keeps its own transform, skew, opacity, and visibility as tracks on the sprite. ramose draws a node's effects in the node's own frame, so `fx shadow 0,20` under `rot:90` puts the shadow to the left of the node. So a baked sprite can turn, scale, and fade in the engine and still match ramose.

A bake draws the clips of every ancestor into its frames, because the engine has no clip for a sprite. If the baked node and a clip move apart, the frames differ, and the bake is a flipbook.

The bake starts at the node that uses a feature the engine cannot draw. It moves up while any ancestor's pixels depend on the subtree in a way the engine cannot draw. That holds for an ancestor that bakes itself, such as one with an `fx` line, `warp:`, a mask, or a brushed fill that clips. It also holds for the enclosing `layer`, instance, or shape with children when a node in it has a `blend:` or a brush that reads `m.below`, since each one reads every sibling below it. Bakes do not nest. The outermost one holds the whole subtree, and it is a flipbook if anything in it changes in any clip.

A node that reads another node's shape bakes with that node's change. If `@x` changes, a node under `inside @x` or `outside @x`, a `mask @x`, and a `copy @x` that cannot take a vertex route all become flipbooks.

A node that bakes in one clip bakes in every clip, so each node has one form in the scene. In a clip where it does not change, its frame track holds one frame.

Each frame of a flipbook covers the union of the subtree's bounds over every clip, in the baked node's frame, so the sprite does not move inside its cell. Each baked node gets one sheet per clip. The sheet draws at the output's scale times the largest scale the node reaches in the world over the clip, so the engine never enlarges a frame. The manifest entry records that scale. #70 can pack the sheets into a shared atlas. If a sheet is larger than the format's texture limit, which is 16384 pixels on a side in Godot, the export stops with an error that names the node and the size.

## The export reports every bake

Every bake is in the export's report, and none is silent. The build prints one line per output with the count of baked nodes. `art-manifest.json` lists each baked node, in paint order. Each entry has the baked node's key as `explain` prints it, `still` or `flipbook`, and its frame count per clip. It also lists each cause, as the key of the node that caused it, the property or feature, and the reason. The reasons are:

| reason | meaning |
|---|---|
| `raster` | The node uses a feature that only ramose's renderer draws. |
| `topology` | The node's mesh does not stay the same over the clip. |
| `blend` | The node composites with its backdrop. |
| `format` | The format lacks the track kind this property routes to. |

## Formats decide which track kinds they carry

The first 2D format is still open. The candidates are a Godot scene, a Spine-style JSON, and sprite sheets only. glTF is the 3D format (#330). Each one carries a set of track kinds, and the route table does not change. The Godot column is measured, except bones and visibility. The other columns come from each format's specification, and the work that picks a format verifies them.

| track kind | Godot `.tscn` | Spine JSON | sprite sheets | glTF |
|---|---|---|---|---|
| transform | yes | bone translate, rotate, scale | no | node translation, rotation, scale |
| skew | yes | bone shear | no | no |
| opacity | yes | slot alpha | no | material alpha, with `KHR_animation_pointer` |
| group opacity | yes | no | no | no |
| visibility | yes | an attachment track | no | `KHR_node_visibility`, with `KHR_animation_pointer` |
| color | yes | slot color | no | material color, with `KHR_animation_pointer` |
| vertex | yes | a deform track on a mesh attachment | no | morph targets |
| bone | yes | yes | no | skins |
| material | yes | no | no | with `KHR_animation_pointer` |
| flipbook | yes | a sequence attachment | yes | one node per frame, toggled with `KHR_node_visibility` |

## Choices this note replaces

- #290 said a patch outside the animatable table is an error. After this note, every change that keeps the scene's structure can be exported, and the route table decides how. Only a structure break is an error (#289). A team that wants no bakes reads the report.
- #290 listed warp parameters as animatable on engine tracks. A warp is not an affine transform, so a `warp:` bakes.
- The issue proposed `modulate` for color. A flat fill exports as the `Polygon2D` `color`, and `modulate` carries only opacity. `modulate` multiplies every descendant, but a ramose fill color belongs to one shape.
