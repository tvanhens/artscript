# artscript core card (draft)

One statement per line: `verb positional… key:value…`, children indented two spaces, `#` comments. An entry file starts with `@target plum.png 1024x1024`. `use face/parts { eye smile }` imports parts. `artscript help <topic>` explains anything in the index.

## Palettes

```
palette plum
  skin ramp #9b3b6a 5   # .0 deep .1 shadow .2 base .3 light .4 highlight
  ink  #3d2632
```

`plum.skin.3` is a step, `plum.skin` the base, `plum.skin.line` its lineart. The house palette is `p` (`p.glint`, `p.rim`). Helpers: `shade(c -1)`, `mix(a b .3)`, `fade(c .5)`, `grad(down a b,.4 c)`, `radial(a b at:.3,.3)`.

## Values

`12 .5 -3`, vectors `14,0` (no spaces at the comma), sizes `40x20`, percents `50%`, lists `[a b]`, ranges `0..8`, references `@body`. Operators take spaces on both sides or neither: `w/2`, `a + b`.

## Shapes

Shapes are centered on their origin, and `at:` places it.

- `curve 0,0 20,-15 40,0`, plus `closed` to fill. `curve pts` takes one list.
- `ellipse 20,12` and `circle 12` take radii. `rect 40x20 r:4`.
- `copy @body at:-20,-8` reuses a named node's outline.
- `subtract`, `union` and `intersect` combine child shapes.

Any node takes `at: rot: scale: a: name: z: blend:`. Paint: `fill:`, `stroke:color,width`, tapered `stroke:p.ink,[2 8 3] taper:.3`, `blur:6`.

## Structure

`layer name` isolates a group, and `group` is plain. **A shape clips its children**, so light and shadow nest inside their form:

```
curve 0,-60 50,-10 30,50 -30,50 -50,-10 closed fill:plum.skin name:body
  ellipse 30,20 at:-20,-30 fill:plum.skin.4 a:.6 blur:12
  subtract fill:plum.skin.1 a:.5 blur:2
    copy @body
    copy @body at:-14,-10
  fx rim 4 dir:1,1
  fx outline 4
```

`fx` applies to its parent: `outline`, `rim`, `grain .05`, `shadow`, `inner`, `glow`, `adjust`. `inside @x` clips a block to another node.

## Logic

`def petal size:20 tone:p.blush` with an indented body, called as `petal 30 at:10,0`. `let w = 34`, `each i in 0..8`, `if`/`else`, `[each k in 0..n -> expr]`. `assert inside(@body canvas margin:24)`.

## Index

targets, privacy, palette from, conic, poly, star, blob, path, xor, mirror, repeat, grid, outside, mask, feather, jitter, dash, cap, join, skew, pivot, fx as/of/merge, fields, seed, anchor, slots, needs, overrides, style, rule, expect, checks, CLI.
