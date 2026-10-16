# Take the small sparkle off the peach

kind: structural
piece: produce/momo_peach

## Request

The little sparkle floating at the lower right clutters the peach's silhouette. Remove it.

## Do

In `art/produce/momo_peach.art`, delete the line `sparkle 20 at:822,770` from `layer sparkles`. Leave `sparkle 52 at:262,360` and `sparkle 28 at:810,330` where they are, at the same size.

## Acceptance

```accept
changed produce/momo_peach.png 811,759 22x22
```
