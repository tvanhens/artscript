# Drop the torii's upper-right maple leaf clear of the beam

kind: structural
piece: monument/vermilion_torii

## Request

The maple leaf beside the right end of the crossbeam crowds that end. Move it lower and a little farther out so it falls clear of the beam, and keep its pivot on the leaf.

## Do

In `art/monument/vermilion_torii.art`, move `layer leaf_b pivot:870,474` and its child `_leaf 870 474 r:36 rot:28 tint:.35` together. Set both the pivot and the `_leaf` position to the same point, target `904,536`, with x between 896 and 912 and y between 528 and 544. Keep `r:36`, `rot:28` and `tint:.35`. Leave `layer leaf_a` and `layer leaf_c` where they are.

## Acceptance

```accept
changed monument/vermilion_torii.png 845,439 94x126
```
