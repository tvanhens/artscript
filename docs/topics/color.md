# color: palettes, ramps, color rows, shade, mix, fade, line, gradients, image paints, sample

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
- Gradients interpolate in OKLCH. `grad(down a b)` runs across the shape's bounds, with directions `down up left right` or an angle (0 = right, 90 = down). `grad(300,240 720,440 a b)` uses those two points as the start and end in the shape's local coordinates. Use `radial(a b at:.3,.3 r:200)` (`at:` is a fraction of the bounds, `r:` is in units) or `conic(a b c)`. Without `r:`, a radial reaches its last stop at the edge of the bounds nearest its center, so `radial(a b)` on a circle ends at the circle's edge. An edge that the center sits on is skipped, so `at:.5,1` reaches the nearer side edge, and past that distance the last stop fills to the corners. A stop can carry a position, as in `p.peach.3,.4`.

Pictures. `image("ref/photo.jpg")` is a paint, and so is `sample("ref/photo.jpg" 240,500)` a color. Both read a PNG or JPEG named relative to the folder with `artscript.toml`. A file that is missing or is not a PNG or JPEG is E0320 at the call. Each file is decoded once per run, and a build is cached on the picture's bytes as well as the sources.

- `fill:image("ref/photo.jpg")` stretches the whole picture over the shape's bounds, as `grad(down ...)` does. `fit:cover` keeps the picture's aspect, scales it to cover the bounds, and crops about the center. `fit:stretch` is the default. The shape's outline crops the picture, and `a:`, `blend:`, `fx` and clipping by nesting act on it as on any fill. `stroke:` takes `image(...)` too, laid over the stroke's bounds. Pixels past the picture's edge repeat its edge pixels. Samples are linear with mipmaps.
- `image(...)` is raster-only. An SVG target bakes the node to a PNG tile, as it does a field fill, and a vector-only target reports it.
- `sample("ref/photo.jpg" x,y)` is the opaque color at picture pixel `x,y`, origin top-left, in picture pixels and not canvas units. It averages the box of `r:` pixels around the pixel (default `r:2`, a 5x5 box, clipped at the picture's edge) in linear light. `r:0` reads one pixel. A position outside the picture is E0320. The result is a plain color, so it works in `let`, as `fill:`, in `mix` and `shade`, and in a gradient. It is not a palette row, which holds only literals.

