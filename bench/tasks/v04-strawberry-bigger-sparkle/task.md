# Enlarge the strawberry's upper-left sparkle

kind: value
piece: produce/sweet_strawberry

## Request

The sparkle at the upper left is too small to read next to the berry. Make that one glint larger, and leave the others where they are.

## Do

In `art/produce/sweet_strawberry.art`, on the top-left sparkle (`sparkle 52 at:232,300 color:berry.twinkle`), raise the size from 52 to between 64 and 70. Leave its position and `color:berry.twinkle` alone.

## Acceptance

```accept
changed produce/sweet_strawberry.png 198,266 68x68
```
