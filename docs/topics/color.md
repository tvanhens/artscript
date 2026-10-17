# color: palettes, ramps, color rows, shade, mix, fade, line, gradients

```
palette garden
  ramp peach #f3a2a6 5        # 0 deep, 1 shadow, 2 base, 3 light, 4 highlight
  ramp leaf #7db956 5 hue:12  # hue: degrees of shift per step
  ramp sky #1b1f3a #f2a65a 7  # two endpoints, interpolated in OKLCH
  color ink #3d2632
```

A palette holds only `ramp` and `color` rows. `ramp <name>` takes one or two colors, an optional stop count (default 5), and an optional `hue:`. `color <name>` takes one color. Outside a palette, `ramp` and `color` are ordinary names, so `color:p.ink` and `ramp(#fff 3)` still work.

A single-color ramp darkens toward cool blue-violet and lightens toward warm yellow, so shadows and lights are hue-shifted for free. `p.peach.line` is the ramp's lineart color, darker and more saturated than the base. `palette night from garden` inherits and overrides. A row name may be a reserved word such as `backdrop` or `if`, because it is only read qualified, as in `p.backdrop`.

- `shade(c k)` steps `k` along `c`'s ramp: `-1` shadow, `-2` deep, `+1` light, `+2` highlight.
- `mix(a b t)` mixes in OKLab, `fade(c a)` sets alpha, `line(c)` is the lineart color for any color.
- Gradients interpolate in OKLCH. `grad(down a b)` runs across the shape's bounds, with directions `down up left right` or an angle (0 = right, 90 = down). `grad(300,240 720,440 a b)` uses those two points as the start and end in the shape's local coordinates. Use `radial(a b at:.3,.3 r:200)` (`at:` is a fraction of the bounds, `r:` is in units) or `conic(a b c)`. A stop can carry a position, as in `p.peach.3,.4`.
