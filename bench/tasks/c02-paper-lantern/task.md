# Add a chochin paper lantern prop

kind: construction
piece: prop/paper_lantern
reads: art/prop/matcha_teacup.art lib/palette/house.art lib/face/parts.art lib/shade/cel.art

## Request

We need a new prop for the set: a chochin paper lantern in the house style. Red and cream ribbed
paper, dark lacquered rings at the top and bottom, a handle to carry it by, and a warm glow from
inside so it reads as lit. Give it a cute face in the spirit of the matcha teacup. It sits on the
ground with a soft shadow and a few sparkles around it, and it has to look like a finished piece of
the set, not a sketch.

## Do

Write a new entry file `art/prop/paper_lantern.art`, 120 to 190 lines, that starts with
`@target prop/paper_lantern.png 1024x1024` and has no other target. `artscript check` exits 0 with
no output, so every color comes from the file's palette or the house palette.

- Its own `palette` with at least a red, a cream and a dark lacquer `ramp`.
- `use face/parts` for `eye`, `blush`, `open_mouth` and `sparkle`.
- The paper is one closed shape named `body`: 400 to 560 wide, 1 to 1.35 times as tall as it is
  wide, centered at x 500 to 524, and `assert inside(@body canvas margin:24)` in the file. Nested
  under `body` so they clip to it:
  - soft blurred light on the upper left and blurred shadow on the lower right or under the caps;
  - a cel shadow, either `use shade/cel { cel }` with `cel @body ...` or a `subtract` of two copies;
  - a cream band across the middle named `band`, clipped to the body, 0.3 to 0.55 of the body's
    height;
  - the ribs, drawn with an `each` loop as horizontal arcs across the body;
  - a warm inner glow, a blurred glow-colored or `blend:screen` shape near the center;
  - at least one tapered accent stroke (`stroke:<color>,[w w ...] taper:`);
  - `fx rim`, `fx grain` and `fx outline` on the body.
- Dark lacquer caps named `cap_top` and `cap_bottom`, each 0.4 to 0.7 of the body's width, centered
  on the body within 6, each with `fx outline`. `cap_top` straddles the body's top edge and
  `cap_bottom` its bottom edge.
- A handle or hanging cord named `handle` that overlaps `cap_top` and rises at least 60 above its
  top.
- The face sits on the band: two `eye`s named `eye_l` and `eye_r`, both inside `band`, their
  midpoint at x 500 to 524, and `assert level(@eye_l @eye_r) tol:.5` in the file; `blush` cheeks
  and an `open_mouth`.
- A ground shadow of blurred ellipses under the base, and at least two `sparkle`s.

## Acceptance

```accept
lines art/prop/paper_lantern.art 120..190
assert art/prop/paper_lantern.art inside(@body canvas margin:24)
assert art/prop/paper_lantern.art level(@eye_l @eye_r) tol:.5
assert art/prop/paper_lantern.art @body.w in 400..560
assert art/prop/paper_lantern.art @body.h / @body.w in 1..1.35
assert art/prop/paper_lantern.art @body.cx in 500..524
assert art/prop/paper_lantern.art @band.h / @body.h in 0.3..0.55
assert art/prop/paper_lantern.art inside(@eye_l @band)
assert art/prop/paper_lantern.art inside(@eye_r @band)
assert art/prop/paper_lantern.art (@eye_l.cx + @eye_r.cx) / 2 in 500..524
assert art/prop/paper_lantern.art @cap_top.w / @body.w in 0.4..0.7
assert art/prop/paper_lantern.art @cap_bottom.w / @body.w in 0.4..0.7
assert art/prop/paper_lantern.art @cap_top.cx - @body.cx in -6..6
assert art/prop/paper_lantern.art @cap_bottom.cx - @body.cx in -6..6
assert art/prop/paper_lantern.art @cap_top.top < @body.top and @cap_top.bottom > @body.top
assert art/prop/paper_lantern.art @cap_bottom.top < @body.bottom and @cap_bottom.bottom > @body.bottom
assert art/prop/paper_lantern.art @handle.top < @cap_top.top - 60
assert art/prop/paper_lantern.art overlaps(@handle @cap_top)
```
