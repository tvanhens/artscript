# craft: structure a piece, layers and names, style, light and value, preview-assisted repairs

## Inspect a repair

Build and open the normal artwork at its intended display size first. Use `preview --crop node --zoom 4 --bg checker --out review/detail.png` for a closer look. Compare `light` and `dark` backgrounds when an edge depends on transparency. Select an exact output with `--target` if the file has several outputs. These controls need no temporary targets or external crop scripts.

When inspection needs different source logic, declare controls such as `param view:one_of(normal geometry) guides:false`. Ordinary conditionals can reveal hidden geometry, replace texture with flat colors, or draw construction lines. Pass overrides to preview and identify the altered view when reporting it. Keep important artwork named so diagnostic additions preserve its random seeds.

For moving attachments, use `--frames 8` or explicit `--times` and open the resulting strip. Compare the loop endpoints explicitly when checking a seam. Fix the source, rebuild without overrides, and inspect the normal shipping output again.

Example: an orange hand must touch a blue arm throughout its swing. The source declares diagnostic controls and draws a joint marker and guides only when they are on:

```
@artboard 128x128
@target arm.png bg:none
@target swing.gif bg:#f4f1ea fps:8 dur:2s clip:swing

param view:one_of(normal geometry) guides:false

palette paint
  color arm #487caa
  color hand #e5a45a
  color ink #203449
  color debug #dd3e98
  color guide #16a6b3

let attachment = 0
let edge = if view == geometry then paint.debug else paint.ink

group arm at:64,64
  rect 48,18 at:0,0 fill:paint.arm stroke:edge,2 name:beam
  circle 10 at:34+attachment,0 fill:paint.hand stroke:edge,2 name:hand
  if view == geometry
    circle 3 at:24,0 fill:paint.debug name:joint
  if guides
    curve -28,0 48,0 stroke:paint.guide,1 name:horizontal
    curve 24,-18 24,18 stroke:paint.guide,1 name:vertical

anim swing dur:2s loop
  .arm rot:key(-30 30 -30)

if clip == swing
  play swing
```

1. Run `ramose check` and `ramose build`, and open `assets/arm.png` at 128 by 128.
2. Suppose `attachment` is 8 and the hand floats free. Locate the joint in the altered view: `ramose preview art/arm.ramo --target arm.png --set view=geometry --set guides=true --crop hand --pad 14 --zoom 4 --bg checker --out review/joint-debug.png`. The guide crosses the arm's endpoint. The receipt records the parameters and the crop.
3. Inspect the normal crop with `--bg light`, `--bg dark`, and `--bg checker`, one `--out` each.
4. Inspect the motion: `--target swing.gif --frames 8 --columns 4` samples the loop evenly and leaves out the repeated endpoint. `--times 0,.5,1,1.5,2` includes both endpoints, so its first and last cells show the same pose and expose a seam.
5. Set `attachment` back to 0, run `check` and `build` with no overrides, and open the normal output and a fresh `--times` sheet. The hand touches the arm in every pose, and neither part clips the artboard.

Report the normal output, the diagnostic paths, and the parameter settings separately.

Structure a piece:

- One entry file per piece, under `art/<category>/<name>.ramo`. Declare its space once with `@artboard 512x512`, and put every output of the piece in its `@target` lines: `@target props/lamp.png` and `@target props/lamp.svg`. A scene that places the piece imports its defs with `use art/props/lamp`.
- Declare a `palette <name>` with named `ramp`s at the top, and paint with `<name>.<ramp>`, `shade(c k)`, and `line(c)`. `p` is the manifest's default palette, never the one in your file, unless the target or an instance writes `palette:elite`, which rebinds `p` under it. Do not write raw hex colors in draws. A ramp gives you the shadow, light, and line colors.
- Put each part in a `layer` named for what it is (`ground_shadow`, `body`, `face`, `details`), drawn back to front. A name is unique among the parts under the same named shape, or at the root of the file, a def, or slot content. A layer or a group opens no scope, so a layer that holds `name:cap` needs another name, such as `cap_layer`. A shape does, so `name:sheen` under `thorax` and again under `abdomen` is legal, and each is reached by its path: `@thorax.sheen`, `@abdomen.sheen`.
- Give the important shapes a `name:`. A name makes the part addressable for `copy @x`, `inside @x`, checks, and later edits.
- Use a `def` for a part that occurs more than once (eyes, leaves, spots). Use `each` and `repeat` instead of copied lines.
- Add `assert` lines for the layout rules the piece depends on, for example `assert inside(@body artboard margin:24)` and `assert level(@eye_l @eye_r) tol:.5`. A check run then catches a later edit that breaks them.

Make it look finished:

- Choose the style first: the one the request names, or the style of the art already in the project, else the one that best serves the subject and its use (an icon that reads small, a scene that sets a mood). State it in one line before you draw, and draw every part in it.
- In every style, hold these. One light with one direction, and every shadow, highlight, and glow agrees with it. A full range of values, with the strongest contrast at the focal point. Forms that overlap, and a silhouette that reads when filled flat. A setting that fits the subject, such as a ground, a sky, or a backdrop, in place of an empty artboard. A margin of about 5% around the piece.
- Nest highlights, shadows, and texture under the shape they belong to. A child is clipped to its parent, so the shading stays inside the form.
- Match the tools to the style. The bundled ones are `use std/brushes { dry chip bristle form }` and `use std/stamps { dots ticks leaves }`.
  - Clean vector or sticker: crisp fills, shading as hard-edged lighter and darker shapes or `brush:form` on a fill, `fx outline` as a main form's last effect, and a soft ground shadow (a dark ellipse with a low `a:` and a large `blur:`).
  - Painterly, such as watercolor, gouache, or oil: few or no outlines. Write a brush for the medium and put it on every painted form; `ramose docs brushes` shows how, and the std brushes are a starting point. A watercolor brush rags the edge, pools pigment darker along it, granulates the wash, and blooms softly, and its washes go on translucent, pale first and darker on top. A gouache or oil brush lays opaque strokes with bristle streaks.
  - Flat graphic: bold shapes, three to five colors, no shading, and no outline or one even weight.
