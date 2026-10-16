# Warm the monk's robe

kind: value
piece: character/moss_monk

## Request

The robe green reads cold beside the gold sash. Warm the cloth toward yellow, and keep it about as light as it is now.

## Do

In `art/character/moss_monk.art`, in `palette monk`, change only the base hex of `moss ramp #7f9a52 5`. Leave the ramp length at 5. In OKLCH the new base keeps lightness between 0.635 and 0.662 and moves hue toward yellow, into 106 to 118 degrees.

## Acceptance

```accept
changed character/moss_monk.png 340,630 120x100
```
