# Raise the lantern's candle flame

kind: value
piece: monument/stone_lantern

## Request

The candle flame in the lantern window is too small to read. Make it taller, and keep it about as wide as it is so it still sits inside the opening.

## Do

In `art/monument/stone_lantern.art`, on the candle flame (`_flame 64 17 3`), raise the height from 64 to between 76 and 84. Leave the width at 17 and the lean at 3.

## Acceptance

```accept
changed monument/stone_lantern.png 491,400 43x89
```
