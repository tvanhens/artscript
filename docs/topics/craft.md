# craft: structure a piece, layers and names, style, light and value, preview-assisted repairs

## Inspect a repair

Build and open the normal artwork at its intended display size first. Use `preview --crop node --zoom 4 --bg checker --out review/detail.png` for a closer look. Compare `light` and `dark` backgrounds when an edge depends on transparency. Select an exact output with `--target` if the file has several outputs. These controls need no temporary targets or external crop scripts.

When inspection needs different source logic, declare controls such as `param view:one_of(normal geometry) guides:false`. Ordinary conditionals can reveal hidden geometry, replace texture with flat colors, or draw construction lines. Pass overrides to preview and identify the altered view when reporting it. Keep important artwork named so diagnostic additions preserve its random seeds.

For moving attachments, use `--frames 8` or explicit `--times` and open the resulting strip. Compare the loop endpoints explicitly when checking a seam. Fix the source, rebuild without overrides, and inspect the normal shipping output again. The complete example in `examples/preview-debug/README.md` follows this workflow.

Structure a piece:

- One entry file per piece, under `art/<category>/<name>.ramo`. Put every output of the piece in its `@target` lines: `@target props/lamp.png 512x512` and `@target props/lamp.svg 512x512`. A scene that places the piece imports its defs with `use art/props/lamp`.
- Declare a `palette <name>` with named `ramp`s at the top, and paint with `<name>.<ramp>`, `shade(c k)`, and `line(c)`. `p` is the manifest's default palette, never the one in your file, unless the target or an instance writes `palette:elite`, which rebinds `p` under it. Do not write raw hex colors in draws. A ramp gives you the shadow, light, and line colors.
- Put each part in a `layer` named for what it is (`ground_shadow`, `body`, `face`, `details`), drawn back to front. A name is unique among the parts under the same named shape, or at the root of the file, a def, or slot content. A layer or a group opens no scope, so a layer that holds `name:cap` needs another name, such as `cap_layer`. A shape does, so `name:sheen` under `thorax` and again under `abdomen` is legal, and each is reached by its path: `@thorax.sheen`, `@abdomen.sheen`.
- Give the important shapes a `name:`. A name makes the part addressable for `copy @x`, `inside @x`, checks, and later edits.
- Use a `def` for a part that occurs more than once (eyes, leaves, spots). Use `each` and `repeat` instead of copied lines.
- Add `assert` lines for the layout rules the piece depends on, for example `assert inside(@body canvas margin:24)` and `assert level(@eye_l @eye_r) tol:.5`. A check run then catches a later edit that breaks them.

Make it look finished:

- Choose the style first: the one the request names, or the style of the art already in the project, else the one that best serves the subject and its use (an icon that reads small, a scene that sets a mood). State it in one line before you draw, and draw every part in it.
- In every style, hold these. One light with one direction, and every shadow, highlight, and glow agrees with it. A full range of values, with the strongest contrast at the focal point. Forms that overlap, and a silhouette that reads when filled flat. A setting that fits the subject, such as a ground, a sky, or a backdrop, in place of an empty canvas. A margin of about 5% around the piece.
- Nest highlights, shadows, and texture under the shape they belong to. A child is clipped to its parent, so the shading stays inside the form.
- Match the tools to the style. The bundled ones are `use std/brushes { dry chip bristle form }` and `use std/stamps { dots ticks leaves }`.
  - Clean vector or sticker: crisp fills, shading as hard-edged lighter and darker shapes or `brush:form` on a fill, `fx outline` as a main form's last effect, and a soft ground shadow (a dark ellipse with a low `a:` and a large `blur:`).
  - Painterly, such as watercolor, gouache, or oil: few or no outlines. Write a brush for the medium and put it on every painted form; `ramose docs brushes` shows how, and the std brushes are a starting point. A watercolor brush rags the edge, pools pigment darker along it, granulates the wash, and blooms softly, and its washes go on translucent, pale first and darker on top. A gouache or oil brush lays opaque strokes with bristle streaks.
  - Flat graphic: bold shapes, three to five colors, no shading, and no outline or one even weight.
