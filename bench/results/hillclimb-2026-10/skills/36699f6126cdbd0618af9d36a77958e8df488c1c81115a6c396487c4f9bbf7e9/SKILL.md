---
name: ramose
description: Draw, edit, and export artwork (sprites, icons, characters, props, animated GIFs) with the ramose CLI. Use when a project needs PNG, SVG, or GIF art made or changed, when the user mentions ramose or `.ramo` files, or when a repo has a `ramose.toml`.
argument-hint: "[what to draw or change]"
allowed-tools: Bash(ramose *) Bash(npx --yes ramose *)
---

# ramose

ramose is a declarative language for procedural art. You write `.ramo` files. `ramose build` renders every `@target` to PNG, SVG, or GIF. Builds are deterministic, so the same source gives the same bytes on the same machine and backend. GPU output can differ from CPU output and between machines.

When the user runs `$ramose:ramose` in Codex or `/ramose` in Claude Code with a request, that request is the art to draw or the change to make. With no request, ask what to draw.

## Before you draw

1. Run `ramose --version`. If it fails, run `npx --yes ramose --version`. If that works, write `npx --yes ramose` everywhere this skill writes `ramose`. If both fail, tell the user to install Node.js and run `npm install -g ramose`, and stop.
2. Run `ramose docs <topic> [topic...]` once per session, naming every topic the task touches. Include `craft` when you draw a new piece or add a part. It prints the core card for the installed version, then those topics. Do not guess syntax from memory. The topics:
   - `targets` @target keys, entry files and libraries, use, the manifest palette p
   - `values` the values table, expressions, operators, functions, fn
   - `color` palettes, ramps, color rows, shade, mix, fade, line, gradients
   - `shapes` the shape table, contour position, keys any node takes, paint keys (fill, stroke, taper, blur, jitter, brush, stamp, pressure, angle, tilt)
   - `structure` layer and group, clipping by nesting, inside/outside, mask, fx, finish
   - `fields` noise fields, fbm, ridged, field fills and masks, seeds
   - `brushes` brush declarations, SkSL bodies, the Mark, brush: on fills, strokes and outlines, channels, stacks, seeds, std/brushes
   - `stamps` stamp declarations, the block and its bound names, spacing and count, stamp: on a node, sets and references, seeds, std/stamps
   - `warp` corners and cylinder, stacks, order of application, the warped box, raster-only
   - `components` def, size box, slots, patches, let/if/each, style
   - `anim` t, key, anim, play, node keys, explain
   - `checks` assert, expect, measurements, check functions, rule
   - `cli` commands, flags, docs topics, exit codes
   - `craft` structure a piece, layers and names, choose a style, light and value, tools for each style
   - `manifest` ramose.toml tables and keys, project defaults, lint levels
3. Find the project root: the nearest `ramose.toml`. If there is none, run `ramose init` in the folder where the art belongs, then delete the starter `art/hello.ramo`. The manifest names the output folder (`out`, default `assets`) and the library roots (`lib`).

## The loop

Before drawing a new piece, choose a concrete art direction in a few sentences. Describe the subject's recognizable silhouette, its placement and overlaps, and the palette and value contrast that draw the eye to the focal point. Fit those choices to the requested style and display size, then use them to guide the drawing.

Repeat until the art is right:

1. Write or change `.ramo` files with the host's file editing tools. Use `apply_patch` in Codex or Write and Edit in Claude Code. Do not edit with `sed`, `python`, or a shell heredoc.
2. Run `ramose check`. Run ramose commands on their own or joined with `&&`, never with `;`, `echo $?`, or other shell plumbing, because the tool already reports the exit code. Fix every error and warning before you continue. Exit codes: 1 compile error, 2 a failed `assert`, 3 warnings only.
3. Run `ramose build --only '<glob>'` for the piece you changed. The glob matches the entry file or an output path, for example `--only 'art/lamp.ramo'` or `--only 'props/lamp*'`. Repeat `--only` to build several pieces, or use `{a,b}`, for example `--only 'art/{lamp,desk}.ramo'`. A glob that matches no target is an error, and `*` does not cross `/`, so use `**` for that.
4. Open the PNG with the host's image viewer, such as `view_image` in Codex or Read in Claude Code, and look at it. Compare it to the request. Look for parts outside the canvas, shading that spills outside its shape, a missing outline, a face that is off-center, and colors that are too close to each other.
5. Run `ramose fmt` on the files you changed.

Never report a piece as done before you have looked at its rendered PNG. For a GIF, also build a still of a mid-animation frame (`t:` on a PNG target) when the motion matters.

## Edit existing art

- Read the whole `.ramo` file before you change it. Run `ramose explain <file>` when the node tree is not clear from the source.
- Change the smallest set of lines that does the job. A color change usually belongs in the palette ramp, not in each draw.
- Do not rename or reorder parts that you do not change. Random seeds come from the file and the node path, so a moved node can draw differently.
- A transparent background is `bg:none` on the target. A solid background is `bg:<color>` on that target.
- An animation is an `anim` block of patch lines, started with `play` at the top of the entry file, and written by a `.gif` target. A GIF needs an opaque `bg:`. Keep the motion small, and loop it so that the last frame matches the first.
- After the edit, run the full loop again and look at every output the edit can change.

## Report back

Tell the user which files you wrote, which outputs `build` wrote, and what you saw when you looked at the PNG. If a check still warns, say which one and why you left it.
