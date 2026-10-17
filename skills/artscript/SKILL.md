---
name: artscript
description: Draw, edit, and export artwork (sprites, icons, characters, props, animated GIFs) with the artscript CLI. Use when a project needs PNG, SVG, or GIF art made or changed, when the user mentions artscript or `.art` files, or when a repo has an `artscript.toml`.
---

# artscript

artscript is a declarative language for vector art. You write `.art` files. `artscript build` renders every `@target` to PNG, SVG, or GIF. Builds are deterministic, so the same source always gives the same bytes.

## Before you draw

1. Run `artscript --version`. If it fails, tell the user that the `artscript` binary must be on PATH, and stop.
2. Run `artscript docs <topic> [topic...]` once per session, naming every topic the task touches. It prints the core card for the installed version, then those topics. Do not guess syntax from memory. The topics:
   - `targets` @target keys, entry files and libraries, use, the manifest palette p
   - `values` the values table, expressions, operators, functions
   - `color` palettes, ramps, color rows, shade, mix, fade, line, gradients
   - `shapes` the shape table, keys any node takes, paint keys (fill, stroke, taper, blur, jitter)
   - `structure` layer and group, clipping by nesting, inside/outside, mask, fx, finish
   - `fields` noise fields, fbm, ridged, field fills and masks, seeds
   - `components` def, size box, slots, patches, let/if/each, style
   - `anim` t, key, anim, play, node keys, explain
   - `checks` assert, expect, measurements, check functions, rule
   - `cli` commands, flags, docs topics, exit codes
3. Find the project root: the nearest `artscript.toml`. If there is none, run `artscript init` in the folder where the art belongs, then delete the starter `art/hello.art`. The manifest names the output folder (`out`, default `assets`) and the library roots (`lib`).

## The loop

Repeat until the art is right:

1. Write or edit the `.art` file.
2. Run `artscript check`. Fix every error and warning before you continue. Exit codes: 1 compile error, 2 a failed `assert`, 3 warnings only.
3. Run `artscript build --only '<glob>'` for the piece you changed. The glob matches the entry file or an output path, for example `--only 'art/lamp.art'` or `--only 'props/lamp*'`. Make sure the output says it built at least one target. A glob that matches nothing builds nothing and still exits 0.
4. Open the PNG with the Read tool and look at it. Compare it to the request. Look for parts outside the canvas, shading that spills outside its shape, a missing outline, a face that is off-center, and colors that are too close to each other.
5. Run `artscript fmt` on the files you changed.

Never report a piece as done before you have looked at its rendered PNG. For a GIF, also build a still of a mid-animation frame (`t:` on a PNG target) when the motion matters.

## Structure a piece

- One entry file per piece, under `art/<category>/<name>.art`. Put every output of the piece in its `@target` lines: `@target props/lamp.png 512x512` and `@target props/lamp.svg 512x512`.
- Declare a `palette <name>` with named `ramp`s at the top, and paint with `<name>.<ramp>`, `shade(c k)`, and `line(c)`. `p` is always the manifest's default palette, never the one in your file. Do not write raw hex colors in draws. A ramp gives you the shadow, light, and line colors.
- Put each part in a `layer` named for what it is (`ground_shadow`, `body`, `face`, `details`), drawn back to front.
- Give the important shapes a `name:`. A name makes the part addressable for `copy @x`, `inside @x`, checks, and later edits.
- Use a `def` for a part that occurs more than once (eyes, leaves, spots). Use `each` and `repeat` instead of copied lines.
- Add `assert` lines for the layout rules the piece depends on, for example `assert inside(@body canvas margin:24)` and `assert level(@eye_l @eye_r) tol:.5`. A check run then catches a later edit that breaks them.

## Make it look finished

- Nest highlights, shadows, and texture under the shape they belong to. A child is clipped to its parent, so the shading stays inside the form.
- A cel shadow is a `subtract` of the shape and an offset copy of it, filled with a darker ramp step at partial alpha.
- A soft highlight is an ellipse with a lighter step at partial alpha and a `blur:`, upper left.
- Finish a main form with `fx outline` as its last effect. Add `fx rim` for a light band and `fx grain` for texture.
- Add a soft ground shadow: a flat ellipse in a dark color with a low `a:` and a large `blur:`, under the piece.
- Leave a margin of about 5% of the canvas around the piece.

## Edit existing art

- Read the whole `.art` file before you change it. Run `artscript explain <file>` when the node tree is not clear from the source.
- Change the smallest set of lines that does the job. A color change usually belongs in the palette ramp, not in each draw.
- Do not rename or reorder parts that you do not change. Random seeds come from the file and the node path, so a moved node can draw differently.
- A transparent background is `bg:none` on the target. A solid background is `bg:<color>` on that target.
- An animation is an `anim` block of patch lines, started with `play` at the top of the entry file, and written by a `.gif` target. A GIF needs an opaque `bg:`. Keep the motion small, and loop it so that the last frame matches the first.
- After the edit, run the full loop again and look at every output the edit can change.

## Report back

Tell the user which files you wrote, which outputs `build` wrote, and what you saw when you looked at the PNG. If a check still warns, say which one and why you left it.
