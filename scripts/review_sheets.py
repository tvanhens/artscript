"""Before/after sheets, a difference map and zoomed crops for each piece, as bench/REVIEW.md asks.
scripts/review_crops.sh renders the inputs and calls this.

Per piece it writes `<name>_light.png` and `<name>_dark.png` (before left, after right, full size),
`<name>_diff.png` (channel difference times 8), and `<name>_crop<n>_<x>_<y>.png` (before above
after, light then dark, 4x nearest-neighbour). Crops are the given regions plus the tiles where the
two renders differ most.
"""

import argparse
import re
from pathlib import Path

from PIL import Image, ImageChops

LIGHT = (255, 255, 255, 255)
DARK = (30, 30, 36, 255)
GUTTER = 16
TILE = 128
AUTO_CROPS = 3
ZOOM = 4
CHANGED_LEVELS = 2


def on(background, image: Image.Image) -> Image.Image:
    ground = Image.new("RGBA", image.size, background)
    return Image.alpha_composite(ground, image)


def side_by_side(images: list[Image.Image], background, vertical: bool = False) -> Image.Image:
    w, h = images[0].size
    size = (w, h * len(images) + GUTTER * (len(images) - 1)) if vertical else (w * len(images) + GUTTER * (len(images) - 1), h)
    sheet = Image.new("RGBA", size, background)
    for i, image in enumerate(images):
        sheet.paste(on(background, image), (0, i * (h + GUTTER)) if vertical else (i * (w + GUTTER), 0))
    return sheet


def difference(before: Image.Image, after: Image.Image) -> Image.Image:
    """Largest channel difference per pixel, as a grayscale image."""
    bands = ImageChops.difference(before, after).split()
    out = bands[0]
    for band in bands[1:]:
        out = ImageChops.lighter(out, band)
    return out


def busiest_tiles(diff: Image.Image, count: int) -> list[tuple[int, int, int, int]]:
    scored = []
    for y in range(0, diff.height - TILE + 1, TILE):
        for x in range(0, diff.width - TILE + 1, TILE):
            score = sum(i * n for i, n in enumerate(diff.crop((x, y, x + TILE, y + TILE)).histogram()))
            if score:
                scored.append((-score, x, y))
    return [(x, y, TILE, TILE) for _, x, y in sorted(scored)[:count]]


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--before", type=Path, required=True)
    parser.add_argument("--after", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--crop", action="append", default=[], help="PIECE:X,Y,WxH")
    parser.add_argument("pieces", nargs="+")
    args = parser.parse_args()

    asked: dict[str, list[tuple[int, int, int, int]]] = {}
    for spec in args.crop:
        piece, x, y, w, h = re.fullmatch(r"(.+):(\d+),(\d+),(\d+)x(\d+)", spec).groups()
        asked.setdefault(piece, []).append((int(x), int(y), int(w), int(h)))

    for piece in args.pieces:
        name = piece.replace("/", "_")
        before = Image.open(args.before / f"{piece}.png").convert("RGBA")
        after = Image.open(args.after / f"{piece}.png").convert("RGBA")
        side_by_side([before, after], LIGHT).save(args.out / f"{name}_light.png")
        side_by_side([before, after], DARK).save(args.out / f"{name}_dark.png")
        diff = difference(before, after)
        diff.point(lambda v: min(255, v * 8)).save(args.out / f"{name}_diff.png")
        histogram = diff.histogram()
        changed = sum(histogram[CHANGED_LEVELS + 1 :]) / (diff.width * diff.height)
        mean = sum(i * n for i, n in enumerate(histogram)) / (diff.width * diff.height)
        print(f"{piece}: {changed * 100:.2f}% of pixels changed by more than {CHANGED_LEVELS} levels, mean max-channel difference {mean:.3f}")

        crops = asked.get(piece, []) + [t for t in busiest_tiles(diff, AUTO_CROPS) if t not in asked.get(piece, [])]
        for n, (x, y, w, h) in enumerate(crops, 1):
            box = (x, y, x + w, y + h)
            pair = [before.crop(box), after.crop(box)]
            column = [side_by_side(pair, LIGHT, vertical=True), side_by_side(pair, DARK, vertical=True)]
            sheet = Image.new("RGBA", (column[0].width * 2 + GUTTER, column[0].height), LIGHT)
            sheet.paste(column[0], (0, 0))
            sheet.paste(column[1], (column[0].width + GUTTER, 0))
            sheet = sheet.resize((sheet.width * ZOOM, sheet.height * ZOOM), Image.NEAREST)
            sheet.save(args.out / f"{name}_crop{n}_{x}_{y}.png")
            print(f"  crop {n}: {x},{y},{w}x{h}")


if __name__ == "__main__":
    main()
