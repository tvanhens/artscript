# Add an acorn forest sprite to the characters

kind: construction
piece: character/acorn_sprite
reads: art/character/moss_monk.art lib/palette/house.art lib/face/parts.art lib/shade/cel.art

## Request

The moss monk needs company. Make a small acorn forest sprite in the house style: a chibi acorn
whose nut is the body, with a big friendly face on it, a scaly cap sitting on top and a little
stalk poking out of the cap. Give it two stubby arms and two little feet, matched on both sides.
Light it like the monk: soft light from the upper left, the crisp shadow step on the far side, a
cool rim, a little grain and inked outlines, with a soft shadow on the ground and a few sparkles.
Draw it fresh rather than reworking the monk. It has to look like a finished piece of the set, not
a sketch.

## Do

Create the entry file `art/character/acorn_sprite.art`, 120 to 190 lines long, that passes
`artscript check` with no output. It must contain all of the following.

- One target line, `@target character/acorn_sprite.png 1024x1024`, and no `.svg` target.
- Its own `palette` block with at least three `ramp` entries, one for the nut, one for the cap and
  one for the stalk or feet. Every color comes from a palette, a ramp step, or a color helper over
  them, so the raw-color lint stays quiet.
- `use face/parts { ... }` importing `eye`, `blush`, `smile` and `sparkle`, and
  `use shade/cel { cel }`.
- The body is one closed shape with `name:nut`. Nested under it: a `cel @nut ...` line, a soft
  light on the upper left (a shape with a light ramp step and `blur:` of 20 or more), a soft
  shadow (a dark ramp step with `blur:` of 12 or more, such as the cap's shadow across the top of
  the nut), and at least one tapered accent stroke, a `curve` with a width list and `taper:`, as in
  `stroke:<color>,[2 6 4] taper:.3`. The body's layer has `fx rim`, `fx grain` and `fx outline`.
- The cap is one closed shape with `name:cap` in a layer drawn after the body, so it covers the
  top of the nut. Its scale pattern is nested under the `cap` line and repeated with `each` or
  `grid`, so every scale clips to the cap. The cap also gets a `cel @cap ...` shadow, and its layer
  has `fx rim` and `fx outline`.
- A stalk with `name:stalk`, a tapered stroke or a shape, rising out of the top of the cap, with
  an `fx outline` in its layer.
- Two `eye` instances with `iris:` taken from the piece's palette: `name:eye_l` with `side:-1`
  and `name:eye_r` with `side:1`. Also two `blush` instances and one `smile`.
- Arms and feet come from two private defs, one for an arm and one for a foot. Each def draws its
  part inside a group mirrored by a `side` parameter, `group at:512,0 scale:side,1` as `_arm` does
  in `moss_monk.art` (or an equivalent `scale:-1,1` flip). Each def is placed twice, with
  `side:1` and `side:-1`, as `name:arm_l`, `name:arm_r`, `name:foot_l` and `name:foot_r`. Every
  layer that holds an arm or a foot has an `fx outline`.
- A ground shadow of blurred ellipses under the feet, in a layer drawn before the feet and the
  body.
- At least two `sparkle` instances.
- `assert inside(@nut canvas margin:24)` and `assert level(@eye_l @eye_r) tol:.5`.
- The composition is new. No coordinate list comes from `moss_monk.art`, so its robe, skull,
  sleeve, hand, foot, ear and leaf curves are absent.
- The geometry matches the acceptance below. The nut is centred, with `@nut.cx` in 492..532,
  `@nut.h` in 400..640, and it is taller than wide. The cap is wider than the nut, its top is
  above the nut's top and its bottom below it. The stalk crosses the cap's top edge. The eyes sit
  below the cap. The eyes, the arms and the feet each mirror about x = 512 within 1. The left arm
  straddles the nut's left edge and is shorter than .3 of the nut's height. The feet reach below
  the nut.

## Acceptance

```accept
lines art/character/acorn_sprite.art 120..190
assert art/character/acorn_sprite.art inside(@nut canvas margin:24)
assert art/character/acorn_sprite.art level(@eye_l @eye_r) tol:.5
assert art/character/acorn_sprite.art @nut.cx in 492..532
assert art/character/acorn_sprite.art @nut.h in 400..640
assert art/character/acorn_sprite.art @nut.w < @nut.h
assert art/character/acorn_sprite.art @cap.w > @nut.w
assert art/character/acorn_sprite.art @cap.top < @nut.top and @cap.bottom > @nut.top
assert art/character/acorn_sprite.art @stalk.top < @cap.top and @stalk.bottom > @cap.top
assert art/character/acorn_sprite.art @eye_l.top > @cap.bottom
assert art/character/acorn_sprite.art symmetric(@eye_l @eye_r about:512) tol:1
assert art/character/acorn_sprite.art symmetric(@arm_l @arm_r about:512) tol:1
assert art/character/acorn_sprite.art symmetric(@foot_l @foot_r about:512) tol:1
assert art/character/acorn_sprite.art @arm_l.left < @nut.left and @arm_l.right > @nut.left
assert art/character/acorn_sprite.art @arm_l.h < @nut.h*.3
assert art/character/acorn_sprite.art @foot_l.bottom > @nut.bottom
```
