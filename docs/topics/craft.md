# craft: structure a piece, layers and names, shading, outline, ground shadow, margin

Structure a piece:

- One entry file per piece, under `art/<category>/<name>.ramo`. Put every output of the piece in its `@target` lines: `@target props/lamp.png 512x512` and `@target props/lamp.svg 512x512`. A scene that places the piece imports its defs with `use art/props/lamp`.
- Declare a `palette <name>` with named `ramp`s at the top, and paint with `<name>.<ramp>`, `shade(c k)`, and `line(c)`. `p` is the manifest's default palette, never the one in your file, unless the target or an instance writes `palette:elite`, which rebinds `p` under it. Do not write raw hex colors in draws. A ramp gives you the shadow, light, and line colors.
- Put each part in a `layer` named for what it is (`ground_shadow`, `body`, `face`, `details`), drawn back to front. A name is unique among the parts under the same named shape, or at the root of the file, a def, or slot content. A layer or a group opens no scope, so a layer that holds `name:cap` needs another name, such as `cap_layer`. A shape does, so `name:sheen` under `thorax` and again under `abdomen` is legal, and each is reached by its path: `@thorax.sheen`, `@abdomen.sheen`.
- Give the important shapes a `name:`. A name makes the part addressable for `copy @x`, `inside @x`, checks, and later edits.
- Use a `def` for a part that occurs more than once (eyes, leaves, spots). Use `each` and `repeat` instead of copied lines.
- Add `assert` lines for the layout rules the piece depends on, for example `assert inside(@body canvas margin:24)` and `assert level(@eye_l @eye_r) tol:.5`. A check run then catches a later edit that breaks them.

Make it look finished:

- Nest highlights, shadows, and texture under the shape they belong to. A child is clipped to its parent, so the shading stays inside the form.
- Shade each main fill with `brush:form` after `use std/brushes { form }`. It lights the shape from the upper left as a rounded volume, from the fill's own color, so a head, body, or cap reads as solid.
- Finish a main form with `fx outline` as its last effect. Add `fx rim` for a light band and `fx grain` for texture.
- Add a soft ground shadow: a flat ellipse in a dark color with a low `a:` and a large `blur:`, under the piece.
- Leave a margin of about 5% of the canvas around the piece.
