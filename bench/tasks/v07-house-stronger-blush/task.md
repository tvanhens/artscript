# Strengthen the house blush

kind: value
piece: produce/momo_peach
reads: art/produce/momo_peach.art

## Request

The blush on the peach is too faint, and the same soft spot is used across the set. Make that shared cheek color read a little stronger, without turning it into paint.

## Do

In `lib/face/parts.art`, on the base ellipse of `def blush` (`ellipse 50%,50% fill:color a:.6 blur:min(size.w size.h)*.2`), raise `a:` from .6 to between .72 and .8. Leave the size, fill, and blur alone. The piece to build and look at is the peach.

## Acceptance

```accept
changed produce/momo_peach.png 322,650 80x46
```
