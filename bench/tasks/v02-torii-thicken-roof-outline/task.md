# Thicken the torii roof outline

kind: value
piece: monument/vermilion_torii

## Request

The dark contour around the curved roof disappears when the gate is shown small. Give that line more weight so the roof still separates from the black sky at thumbnail size.

## Do

In `art/monument/vermilion_torii.art`, on the roof union in `layer kasagi` (`fx outline 7 color:gate`), raise the outline width from 7 to between 9 and 11. Leave `color:gate` alone.

## Acceptance

```accept
changed monument/vermilion_torii.png 132,200 700x74
```
