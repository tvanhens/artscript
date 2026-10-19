# bookshelf

Two pieces painted against a reference photo.

`art/narnia.art` redraws `ref/narnia.jpg`, a CC0 photo by MorningbirdPhoto from [Wikimedia Commons](https://commons.wikimedia.org/wiki/File:Books-bookshelf-bookstore-159778.jpg). The photo is a reference only. `narnia.png` draws every part with shapes, text, gradients and brushes. The `onion` target draws the same scene with the photo at half opacity on top, for checking registration by eye.

`art/bookcase.art` reuses the matched spine as the `book` component in `lib/books.art`. It fills three shelves from lists of titles, authors and cloth colors. Six of the cloth colors are sampled from the photo.

## The loop

1. Pick colors with `sample("ref/narnia.jpg" x,y r:3)`. Each spine's fill is a gradient of five samples down one column of the photo, so it carries the photo's light falloff.
2. Measure with `artscript compare art/narnia.art --ref ref/narnia.jpg --target narnia.png`. It prints the mean ΔE, the worst named parts with the color change each needs, and the worst grid cells. It also writes a sheet with the reference, the render, an overlay and a ΔE heatmap.
3. Fit the type with `python3 tools/fit_type.py 4`. It rewrites the `spine` lines from `tools/spines.json`. Each round moves one type parameter on every spine and keeps the move only on the spines where ΔE inside that spine drops.

Keep a change only when `compare` says it lowered ΔE.
