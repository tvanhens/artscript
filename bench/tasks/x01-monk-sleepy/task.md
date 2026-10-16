# Give the moss monk a sleepy pose

kind: variant
piece: character/moss_monk
reads: lib/face/parts.art
looks: character/moss_monk_sleepy

## Request

The moss monk needs a sleepy portrait next to the blink and the yawn. Let the lids droop without shutting the way the blink does, shrink the smile into a small drowsy mouth, and sag the brows. Add a small sleep bubble off the upper left of the head so the pose reads at a glance. Leave the plain portrait as it is.

## Do

In `art/character/moss_monk.art`, add `@target character/moss_monk_sleepy.png 1024x1024 as:sleepy` beside the yawn target.

On the brow in `layer eyes` (the curve that starts `curve 512+120*s,362`), when `target == sleepy` lower the outer point by 10 to 18, the middle point by 14 to 22, and the inner point by 4 to 10, so the brow slopes down toward the outside of the face. On every other target those y values stay 362, 348, and 354. Leave the stroke alone.

On the eye that starts `eye 104x112 iris:iris`, set `lid:` to 1 when `target == blink`, to between .82 and .92 when `target == sleepy`, and to .6 otherwise. Leave the size, place, iris, and `look:` alone.

In `layer mouth`, keep `o_mouth 36 at:512,508` for the yawn. When `target == sleepy`, draw `smile` with `color:monk.mouth`, size 20 to 32, and `at:512` with y from 508 to 520. Otherwise keep `smile 54 at:512,504`.

When `target == sleepy`, add `layer sleep_bubble` at the upper left of the head, and do not draw that layer for any other target. Build it from three `monk.star` ellipses. The large one is about 72 by 48 and sits within 40 pixels of `208,242`, with a soft upper-left glint. Two smaller ellipses trail from it toward the temple. Inside the large ellipse, draw a Z from three tapered `monk.shadow` curves.

## Acceptance

```accept
unchanged character/moss_monk.png 0,0 1024x1024
unchanged character/moss_monk_blink.png 0,0 1024x1024
differs character/moss_monk_sleepy.png character/moss_monk.png 134,192 176x158
differs character/moss_monk_sleepy.png character/moss_monk.png 390,343 244x35
differs character/moss_monk_sleepy.png character/moss_monk.png 359,420 310x70
differs character/moss_monk_sleepy.png character/moss_monk.png 478,496 68x30
```
