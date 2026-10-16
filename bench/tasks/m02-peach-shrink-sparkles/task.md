# Shrink the peach's three sparkles

kind: multi
piece: produce/momo_peach

## Request

The three sparkles compete with the peach's face. Shrink each of them by about a quarter, and leave them where they are.

## Do

In `art/produce/momo_peach.art`, inside `layer sparkles`, lower each sparkle's size by about a quarter and keep its `at:` point.

`sparkle 52 at:262,360` becomes size 39, between 37 and 41, still `at:262,360`.

`sparkle 28 at:810,330` becomes size 21, between 20 and 22, still `at:810,330`.

`sparkle 20 at:822,770` becomes size 15, between 14 and 16, still `at:822,770`.

Leave color and glow on their defaults.

## Acceptance

```accept
changed produce/momo_peach.png 235,333 54x54
changed produce/momo_peach.png 795,315 30x30
changed produce/momo_peach.png 811,759 22x22
```
