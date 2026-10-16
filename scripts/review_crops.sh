#!/usr/bin/env bash
# Render the before/after sheets and crops an art review needs (bench/REVIEW.md).
# Usage: scripts/review_crops.sh [--before REF] [--out DIR] [--crop PIECE:X,Y,WxH]... PIECE...
#   PIECE is a target path without extension, such as produce/momo_peach.
#   Before is examples/chibi at REF (default HEAD), rendered by artscript built from REF. After is
#   the working tree, rendered by artscript built from the working tree, twice from clean copies so
#   the two builds can be compared byte for byte.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
before_ref=HEAD
out=/tmp/artscript-review
crops=()
pieces=()
while [ "$#" -gt 0 ]; do
	case "$1" in
		--before) before_ref="$2"; shift 2 ;;
		--out) out="$2"; shift 2 ;;
		--crop) crops+=("$2"); shift 2 ;;
		-*) printf 'unknown flag %s\n' "$1" >&2; exit 1 ;;
		*) pieces+=("$1"); shift ;;
	esac
done
if [ "${#pieces[@]}" -eq 0 ]; then
	sed -n '2,7p' "$0" >&2
	exit 1
fi

engine=(crates Cargo.toml Cargo.lock)
cargo build --quiet --release -p artscript --manifest-path "$root/Cargo.toml"
after_bin="$root/target/release/artscript"
before_bin="$after_bin"
if ! git -C "$root" diff --quiet "$before_ref" -- "${engine[@]}" ||
	[ -n "$(git -C "$root" ls-files --others --exclude-standard -- "${engine[@]}")" ]; then
	before_src="$root/target/review-before/src"
	rm -rf "$before_src"
	mkdir -p "$before_src"
	git -C "$root" archive "$before_ref" "${engine[@]}" | tar -x -C "$before_src"
	cargo build --quiet --release -p artscript --manifest-path "$before_src/Cargo.toml" \
		--target-dir "$root/target/review-before/target"
	before_bin="$root/target/review-before/target/release/artscript"
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/before"
git -C "$root" archive "$before_ref" examples/chibi | tar -x -C "$work/before" --strip-components=2
for copy in after_1 after_2; do
	cp -r "$root/examples/chibi" "$work/$copy"
done
for copy in before after_1 after_2; do
	rm -rf "$work/$copy/assets" "$work/$copy/.artscript"
	bin="$after_bin"
	[ "$copy" = before ] && bin="$before_bin"
	for piece in "${pieces[@]}"; do
		(cd "$work/$copy" && "$bin" build --only "$piece.png" > /dev/null)
	done
done

rm -rf "$out"
mkdir -p "$out"
for piece in "${pieces[@]}"; do
	if cmp -s "$work/after_1/assets/$piece.png" "$work/after_2/assets/$piece.png"; then
		printf '%s: two clean builds are byte-identical\n' "$piece" >> "$out/report.txt"
	else
		printf '%s: two clean builds DIFFER\n' "$piece" >> "$out/report.txt"
	fi
done

crop_args=()
for c in "${crops[@]}"; do
	crop_args+=(--crop "$c")
done
uv run --with pillow python "$root/scripts/review_sheets.py" \
	--before "$work/before/assets" --after "$work/after_1/assets" --out "$out" \
	"${crop_args[@]}" "${pieces[@]}" >> "$out/report.txt"
cat "$out/report.txt"
printf 'sheets and crops in %s\n' "$out"
