<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/logo-dark.png">
    <img src="assets/logo-paper.png" alt="The artscript logo: a pebble with a coral corner mark over the lowercase wordmark" width="300">
  </picture>
</p>

<p align="center">
  <b>Art assets your LLM can write, read, and fix.</b><br>
  A compact language for procedural art, and a CLI that builds it to PNG, SVG, and GIF.
</p>

<p align="center">
  <a href="#get-started"><img src="https://img.shields.io/badge/install-cargo%20install-1f2328?style=flat" alt="Install with cargo"></a>
  <a href="rust-toolchain.toml"><img src="https://img.shields.io/badge/rust-stable-1f2328?style=flat" alt="Stable Rust"></a>
  <a href="docs/language.md"><img src="https://img.shields.io/badge/status-v0.1%20draft-ff6b5b?style=flat" alt="v0.1 draft"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-BSL%201.1-1f2328?style=flat" alt="Business Source License 1.1"></a>
</p>

<p align="center"><sub>
  <a href="#the-bogmaw">The bogmaw</a> ·
  <a href="#built-for-the-models-side-of-the-loop">Why</a> ·
  <a href="#one-file-three-slimes">A whole file</a> ·
  <a href="#the-agent-loop">Agent loop</a> ·
  <a href="#get-started">Get started</a>
</sub></p>

<p align="center">
  <img src="examples/spire/assets/monster/bogmaw_card.png" alt="The bogmaw: a moss-covered, beetle-shelled monster with three amber eyes, a jaw of bone teeth, spined plates, and glowing mushrooms on its back" width="640">
</p>

<p align="center"><sub>The bogmaw. About 200 lines of artscript, built to a 1024px PNG. No pixel was painted by hand.</sub></p>

---

A game needs hundreds of creatures, props, and icons, and every one of them needs variants. A model can draw one of them in SVG. It struggles to draw the hundredth in the same style, to change one detail without regenerating the file, or to tell whether its own output is right.

artscript gives the model a language it can hold in one turn: palettes, components, brushes, and checks, in plain indented text. The CLI renders it, and it reports what is wrong with a line number and a fix.

## The bogmaw

[`examples/spire`](examples/spire) is one painted monster, written as a project. This is its whole palette:

```
palette bog
  ramp shell #4f7d6b 7 hue:6
  ramp belly #c49a7c 5
  ramp flesh #8a4a52 5
  ramp maw #3a1424 5
  ramp bone #e3d5b0 5
  ramp moss #7f9a3c 5
  ramp glow #ffb43a 5
  color ink #1b1420
  color shine #fff4d6
  color rim #9fd6e0
  color backdrop #26232c
  ramp rust #b8643a 5
  ramp leg #5d6b45 7 hue:8
```

Each `ramp` turns one color into a painted range, with cool shadows and warm lights. Every stroke in the picture picks a stop from this list, so a recolor is a palette edit.

The far legs are the same `leg` component as the near ones. One `fx` line pushes them back into the shade:

```
layer legs_far
  leg 640,610 715,705 690,850 w:23 tone:p.leg.2 bend:-6 spur:0 name:leg_f1
  leg 770,615 860,690 850,835 w:21 tone:p.leg.2 bend:-10 name:leg_f2
  leg 870,590 950,680 925,848 w:18 tone:p.leg.2 bend:-8 spur:0 name:leg_f3
  fx adjust hue:-14 light:-.14 sat:-.25
  fx rim 3 dir:1,-1 color:p.rim a:.35
  fx outline 4 color:p.shell.0
```

The moss is a `stamp`, a block of drawing that artscript lays along a curve again and again. Each copy draws a few circles with their own seeded sizes and shades. The shell plates use `sculpt`, a brush from the project's library, which shades a flat fill into a lit, glossy form.

`bogmaw.art` is 5,438 tokens. The brushes, parts, and palette it imports add 2,397.

## More from the same language

<table>
  <tr>
    <td width="33%" align="center"><img src="examples/slime/assets/slime_grape.gif" alt="A grape-purple jelly slime squashing and stretching"></td>
    <td width="33%" align="center"><img src="examples/chibi/assets/character/moss_monk_idle.gif" alt="A chibi monk with a leaf sprouting from its head, bobbing gently"></td>
    <td width="33%" align="center"><img src="examples/chibi/assets/monument/stone_lantern.png" alt="A mossy stone lantern"></td>
  </tr>
  <tr>
    <td align="center"><sub><a href="examples/slime">slime</a>: three flavors and three loops from one 435-token file</sub></td>
    <td align="center"><sub><a href="examples/chibi">chibi</a>: an animated character built from a shared face-parts library</sub></td>
    <td align="center"><sub><a href="examples/chibi">chibi</a>: props and monuments in one house style</sub></td>
  </tr>
</table>

## Built for the model's side of the loop

**Small enough to write in one turn.** The core language card, [`docs/language.md`](docs/language.md), is 988 tokens. `artscript docs <topic>` adds only the topics a task needs. There are fourteen.

**Edits stay local.** Style lives in palettes, components, and named parts, not in baked coordinates. To give the chibi monk blue eyes, a model changes one line:

```diff
-  color iris #9a6a3a
+  color iris #4f7fc0
```

Rebuilt, only the eyes change. Every other pixel is identical. `artscript fmt` prints one canonical form, so diffs carry only what changed.

**Feedback a model can act on.** `artscript check` evaluates a whole project without rendering. `--json` gives the agent the location and a fix:

```json
{
  "code": "C0101",
  "file": "art/slime.art",
  "fix": "set .eye.1 at:50,-114",
  "message": "assert level(@eye.0 @eye.1)",
  "node": "",
  "notes": ["eye.cy = 266   eye.cy = 274   difference 8 > tol 0.5"],
  "severity": "error",
  "span": { "col": 1, "line": 33 }
}
```

**Eyes optional.** A model doesn't need to look at a render to know where things are. `assert` and `expect` lines encode the composition rules: eyes level, the body inside the canvas with a margin, the head as wide as the body. `artscript explain` prints every node with its bounds and resolved colors as text.

**Made for big projects.** Shared palettes and component libraries (`use parts { leg fang }`) keep a house style across files. One `@target` line fans out every variant in a matrix. `build --only 'monster/*'` lets each agent render just the files it owns. Builds are deterministic on one machine and backend, and `art-manifest.json` records a hash per output, so a reviewer sees exactly which assets changed.

## One file, three slimes

This is all of [`examples/slime/art/slime.art`](examples/slime/art/slime.art). It builds three PNGs and three animated GIFs:

```
@target slime_{flavor}.png 512x512 each flavor:[mint grape peach]
@target slime_{flavor}.gif 240x240 bg:#fbf6ee fps:24 each flavor:[mint grape peach]

palette goo
  ramp mint #7fd4a8 5
  ramp grape #b49cf0 5
  ramp peach #f7a98c 5
  color ink #2b2440
  color shine #ffffff

let jelly = if flavor == grape then goo.grape else if flavor == peach then goo.peach else goo.mint

layer slime scale:canvas.w/512
  ellipse 160,22 at:256,392 fill:goo.ink a:.2 blur:10
  group at:256,380 name:squash
    curve -150,0 -140,-120 -60,-220 60,-220 140,-120 150,0 closed tension:.5 fill:grad(down jelly.3 jelly.1) name:body
      ellipse 50,30 at:-60,-170 rot:-30 fill:jelly.4 a:.8 blur:8
      subtract fill:jelly.0 a:.4 blur:3
        copy @body
        copy @body at:-24,-20
      each s in [-1 1]
        circle 18 at:50*s,-110 fill:goo.ink name:eye
          circle 6 at:-6,-6 fill:goo.shine
      curve -22,-70 0,-56 22,-70 stroke:goo.ink,[3 6 3] taper:.3
      fx rim 6 dir:1,1 color:goo.shine a:.4
      fx outline 6 color:jelly.line

anim wobble dur:1.2s loop
  .squash scale:key(1,1 1.08,.92 .96,1.04 1,1)

play wobble

assert level(@eye.0 @eye.1)
assert inside(@body canvas margin:24)
```

<p align="center">
  <img src="examples/slime/assets/slime_mint.gif" alt="A mint-green jelly slime squashing and stretching" width="200">
  <img src="examples/slime/assets/slime_grape.gif" alt="A grape-purple jelly slime squashing and stretching" width="200">
  <img src="examples/slime/assets/slime_peach.gif" alt="A peach-orange jelly slime squashing and stretching" width="200">
</p>

- **One color in, a painted ramp out.** `ramp mint #7fd4a8 5` makes five stops whose shadows shift cool and whose lights shift warm. `jelly.line` is a matching outline color. A new flavor is one palette row.
- **Children clip to their parent.** The highlight, the shadow, and the eyes sit under `body`, so none of them can spill outside it, whatever the model gets wrong about coordinates.
- **Shapes are values.** The shadow is the body minus a nudged copy of itself. Reshape the body and the shadow follows.
- **Motion is a patch.** `anim wobble` scales the `squash` group over time. The PNG draws the rest pose, and the GIF plays the loop.
- **The rules travel with the art.** Tilt the eyes with `at:50*s,-110+4*s` and `check` exits 2 with the diagnostic shown above. It checks every GIF frame too, and it reports the worst moment.

## The agent loop

1. Give the model the output of `artscript docs <topic>...` for the topics the task touches, and the project's palette and component libraries.
2. The model writes or edits an `.art` file.
3. Run `artscript check --json`. Feed any diagnostics back, and repeat until it exits 0.
4. Run `artscript build --only '<its files>'` to render.
5. Review the source diff and the changed hashes in `art-manifest.json`.

## What the language covers

| | |
|---|---|
| **Shapes** | `rect`, `circle`, `ellipse`, `poly`, `star`, `curve`, `blob`, SVG `path`, booleans (`subtract`, `union`, `intersect`, `xor`), `mirror`, `repeat`, `grid` |
| **Ink** | Tapered brush strokes with one width per point, hand-drawn `jitter`, round caps and joins |
| **Brushes and stamps** | SkSL brushes that shade a fill or a stroke, and stamps that lay a block of drawing along a path |
| **Color** | Hue-shifted ramps, OKLCH gradients (linear, radial, conic), `shade`, `mix`, `fade` |
| **Texture** | `simplex`, `perlin`, `worley`, and `fbm` noise fields, as fills or as masks |
| **Effects** | `rim`, `outline`, `glow`, `shadow`, `inner`, `grain`, `blur`, `adjust`, and named `finish` stacks you reuse |
| **Components** | `def` with typed parameters, slots, anchors, and `.part` overrides, shared across files with `use` |
| **Motion** | `anim` clips with keyframes and easing, played into GIFs |
| **Output** | PNG at any `@2x` scale, SVG, GIF, and `art-manifest.json` |
| **Checks** | `assert` and `expect` on position, size, alignment, symmetry, overlap, and stroke width |

## Get started

artscript builds with stable Rust.

```sh
git clone https://github.com/tvanhens/artscript
cd artscript
cargo install --path crates/cli
```

Make a project, then build it:

```sh
mkdir my-art && cd my-art
artscript init
artscript build
```

`init` writes a manifest, a starter palette, and `art/hello.art`. `build` writes `assets/hello.png`, `assets/hello@2x.png`, and `assets/art-manifest.json`.

| Command | What it does |
|---|---|
| `artscript build [--only GLOB]...` | Render every `@target` and write the manifest |
| `artscript check [--json]` | Evaluate every target and run its checks, without rendering |
| `artscript fmt [--check]` | Rewrite `.art` files in the canonical style |
| `artscript explain <file>` | Print the evaluated node tree |
| `artscript docs [TOPIC]...` | Print the language card, or the named topics |
| `artscript init` | Create a starter project |

## Learn more

- [`docs/language.md`](docs/language.md) is the core language card, and [`docs/topics/`](docs/topics) holds one file per topic. `artscript docs` prints both.
- [`examples/spire`](examples/spire) is the bogmaw: custom brushes, stamps, and a parts library for a painted monster.
- [`examples/chibi`](examples/chibi) is a full project with a shared palette, a face-parts library, and an animated character.
- [`assets/logo.art`](assets/logo.art) draws the logo above. The wordmark is set in [Inter](https://rsms.me/inter/), under the SIL Open Font License.

artscript is a v0.1 draft. Expect breaking changes.

## License

artscript is source-available under the [Business Source License 1.1](LICENSE).

- **Free.** Use artscript to make art for your own games, apps, and other projects, commercial ones included. The images and animations you build are yours, and the license places no restriction on them.
- **Needs a commercial license.** Offering artscript to other people as part of a platform: a hosted or API asset generator, a game engine or editor that includes it, or an AI product that builds artscript for its users. [Contact the author](https://github.com/tvanhens) for terms.

Each version becomes available under the Apache License 2.0 four years after its release. The [`LICENSE`](LICENSE) file has the full terms.
