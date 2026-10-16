# Quiet the lantern so it glows like a candle

kind: multi
piece: monument/stone_lantern

## Request

The lantern throws light like a lamp. Quiet it so the fire reads as a candle. Lower the glow around the window, the light pooled inside the window, and the warm spill on the shelf under the firebox.

## Do

In `art/monument/stone_lantern.art`, lower only these three alphas. Leave size, position, color, and blur alone.

On `circle 160 at:512,452` inside `layer glow`, the fill is `radial(fade(lantern.amber.3 .42) fade(lantern.amber .16),.45 fade(lantern.amber 0) r:160)`. Bring the `.42` fade alpha to between .06 and .12, and the `.16` fade alpha to between .02 and .05.

On `copy @window` inside that same `layer glow`, the fill is `radial(fade(lantern.amber.4 .75) fade(lantern.amber.3 .3),.6 fade(lantern.amber 0) at:.5,.64 r:66)`. Bring the `.75` fade alpha to between .14 and .24, and the `.3` fade alpha to between .05 and .10.

On `ellipse 86,16 at:512,552 fill:lantern.amber.3 a:.35 blur:10`, bring `a:` from .35 to between .06 and .12.

Leave the window's own fill, the flame, and the soft-light ellipse on the firebox as they are.

## Acceptance

```accept
changed monument/stone_lantern.png 356,368 44x80
changed monument/stone_lantern.png 468,400 90x100
changed monument/stone_lantern.png 472,546 80x14
```
