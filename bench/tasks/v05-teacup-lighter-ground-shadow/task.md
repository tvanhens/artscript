# Lighten the shadow under the teacup

kind: value
piece: prop/matcha_teacup

## Request

The shadow under the saucer is too heavy, so the cup looks stuck to the ground. Lighten that broad shadow and leave the tighter one just under the saucer as it is.

## Do

In `art/prop/matcha_teacup.art`, on the outer ground-shadow ellipse (`ellipse 330,40 at:512,868 fill:fade(tea.shadow .38) blur:16`), lower the alpha in `fade(tea.shadow .38)` from .38 to between .26 and .30. Leave the ellipse size, position, blur, and the inner shadow ellipse alone.

## Acceptance

```accept
changed prop/matcha_teacup.png 300,900 420x28
```
