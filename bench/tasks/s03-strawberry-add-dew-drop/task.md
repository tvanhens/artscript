# Add a smaller dew drop on the strawberry's right side

kind: structural
piece: produce/sweet_strawberry

## Request

The berry already carries one dew drop on its lower left. Add a second, smaller drop on the right half. Keep it on the skin, and keep it off the eye, the blush, and the seeds.

## Do

In `art/produce/sweet_strawberry.art`, inside the skin curve and on the line after the existing `_dew`, add `_dew 736 654 r:9 name:dew_right`. Keep the new drop's x between 730 and 742, its y between 648 and 656, and its `r:` between 8 and 10. Leave the first `_dew` on its defaults.

## Acceptance

```accept
changed produce/sweet_strawberry.png 727,639 23x32
assert art/produce/sweet_strawberry.art @dew_right.cx in 731..745
assert art/produce/sweet_strawberry.art @dew_right.cy in 648..657
assert art/produce/sweet_strawberry.art inside(@dew_right @skin)
assert art/produce/sweet_strawberry.art @dew_right.w < 26
```
