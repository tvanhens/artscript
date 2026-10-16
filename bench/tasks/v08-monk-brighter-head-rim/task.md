# Brighten the rim light on the monk's head

kind: value
piece: character/moss_monk

## Request

The cool light along the edge of the head is too weak to see. Bring that rim up so the head catches it, and leave the rim on the robe as it is.

## Do

In `art/character/moss_monk.art`, on `layer head`'s rim (`fx rim 5 dir:1,1 color:rim a:.45`, the one just above `fx grain .03 scale:2`), raise `a:` from .45 to between .6 and .7. Leave the width, `dir:1,1`, and `color:rim` alone. Leave the robe layer's matching rim at .45.

## Acceptance

```accept
changed character/moss_monk.png 640,530 80x60
```
