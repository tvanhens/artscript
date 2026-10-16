# Soften the peach's upper-left highlight

kind: value
piece: produce/momo_peach

## Request

The cream highlight on the peach's upper left reads as a hard spot sitting on the skin. Let it
melt into the fuzz so it looks like light on velvet, not a sticker.

## Do

In `art/produce/momo_peach.art`, on the body's cream highlight (`ellipse 80,56 at:352,470
fill:peach.cream.4`), raise `blur:` from 30 to between 40 and 50 and lower `a:` from .55 to
between .38 and .48. Leave its size, position and color alone.

## Acceptance

```accept
changed produce/momo_peach.png 262,404 180x132
```
