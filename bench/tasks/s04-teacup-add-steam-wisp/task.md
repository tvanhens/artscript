# Add a shorter steam wisp between the teacup's two

kind: structural
piece: prop/matcha_teacup

## Request

The cup sends up two wisps of steam. Add a third, shorter wisp between them, rising from the rim the way the other two do.

## Do

In `art/prop/matcha_teacup.art`, between `layer steam_l` and `layer steam_r`, add a layer that calls `_wisp`.

```
layer steam_c pivot:514,466
  _wisp [514,464 492,420 522,378 500,346] w:[5 12 10 3] n:4
```

Keep the pivot within 8 of `514,466` and within 4 of the wisp's first point. Keep each point within 8 of the values above, so every x stays between 484 and 530 and the highest point's y stays between 338 and 354. That top sits lower than `steam_r`'s tip at y=292 and `steam_l`'s tip at y=206. Keep each width within 2 of `[5 12 10 3]`, and keep `n:4`. Leave `steam_l` and `steam_r` unchanged.

## Acceptance

```accept
changed prop/matcha_teacup.png 483,344 58x127
assert art/prop/matcha_teacup.art @steam_c.h < @steam_r.h
assert art/prop/matcha_teacup.art @steam_c.top > @steam_r.top
assert art/prop/matcha_teacup.art @steam_c.h < @steam_l.h
```
