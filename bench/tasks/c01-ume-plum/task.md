# Add an ume plum to the produce set

kind: construction
piece: produce/ume_plum
reads: art/produce/momo_peach.art art/produce/sweet_strawberry.art lib/palette/house.art lib/face/parts.art lib/shade/cel.art

## Request

We need a plum in the house style for the produce shelf. Make it an ume, the Japanese plum. It
should be a round, ripe little fruit, yellow-green with a rosy cheek, with a cute face and a short
stem with a leaf. It sits on the same ground line as the peach and the strawberry and should look
like it shipped with them. That means soft light from the upper left, the same crisp shadow step
on the far side, inked outlines, a soft shadow on the ground and a few sparkles. Draw it fresh and
don't trace the peach. It has to look finished, not like a sketch.

## Do

Create the entry file `art/produce/ume_plum.art`, 120 to 190 lines long, that passes
`artscript check` with no output. It must contain all of the following.

- One target line, `@target produce/ume_plum.png 1024x1024`, and no `.svg` target.
- Its own `palette` block with at least three `ramp` entries, one for the body, one for the leaf
  and one for the stem. Every color comes from a palette, a ramp step, or a color helper over
  them, so the raw-color lint stays quiet.
- `use face/parts { ... }` importing an eye part (`eye` or `happy_eye`), a mouth part (`smile`,
  `cat_mouth`, `open_mouth` or `o_mouth`), `blush` and `sparkle`. Two eyes placed with
  `name:eye_l` and `name:eye_r`, one mouth, and two `blush` instances.
- The body is one closed shape with `name:skin`, with outline points of its own. No coordinate
  list comes from the peach, so its `skin_pts`, its twig curves and its `_leaf` blade are absent.
- Nested under the `skin` line: a soft light on the upper left (a shape with a light ramp step and
  `blur:` of 20 or more), a soft shadow on the lower right or bottom (a dark ramp step, `blur:` of
  20 or more), and a cel shadow. The cel shadow is either `use shade/cel { cel }` with a
  `cel @skin ...` line nested under `skin`, or a `subtract` of `copy @skin` and a shifted
  `copy @skin`.
- At least one tapered accent stroke on the body, a `curve` with a width list and `taper:`, as in
  `stroke:<color>,[3 4 2] taper:.3`.
- `fx rim`, `fx grain` and `fx outline` lines on `skin`.
- A stem (a tapered stroke or a shape) and at least one leaf, each with its own `fx outline`.
- A ground shadow of blurred ellipses below the body, in a layer drawn before the body layer.
- At least two `sparkle` instances.
- `assert inside(@skin canvas margin:24)` and `assert level(@eye_l @eye_r) tol:.5`.
- The body matches the set's scale. `@skin.w` is in 480..620 and `@skin.h` in 440..560. An ume
  is round, so `@skin.w/@skin.h` is in .95..1.12, where the peach is a squat 1.17. Its centre
  is near the canvas middle, with `@skin.cx` in 482..542 and `@skin.cy` in 560..650. Its bottom
  edge, `@skin.bottom`, is in 830..880, since the peach and the strawberry both end near 851.
  Both eyes sit inside `skin` and below its centre.

## Acceptance

```accept
lines art/produce/ume_plum.art 120..190
assert art/produce/ume_plum.art @skin.w in 480..620
assert art/produce/ume_plum.art @skin.h in 440..560
assert art/produce/ume_plum.art @skin.w/@skin.h in .95..1.12
assert art/produce/ume_plum.art @skin.cx in 482..542
assert art/produce/ume_plum.art @skin.cy in 560..650
assert art/produce/ume_plum.art @skin.bottom in 830..880
assert art/produce/ume_plum.art inside(@eye_l @skin)
assert art/produce/ume_plum.art inside(@eye_r @skin)
assert art/produce/ume_plum.art @eye_l.cy > @skin.cy
```
