# Art director review: artscript for a metroidvania asset pipeline

This review covers a playtest of artscript 0.1.0 (`e505ad0`). Ten agents used the tool to build the asset set for a 2D metroidvania, working title *Hollow Lantern*:

- a modular hero rig
- six enemies and an NPC, each built from separate parts
- tile, prop and parallax kits for three biomes
- a UI set
- three composed scenes, in [`scenes/`](scenes/)

The playtest filed issues #51 to #86.

## Rating: 5 out of 10

The ideas behind the tool rate a 7 or 8. The tool as it works today rates about a 4. We would not adopt it for production yet.

### What works

- **Builds are deterministic.** Two builds gave byte-identical results for about 150 outputs.
- **One style held across ten parallel authors.** The scenes share one outline weight, one light direction and one palette. Frost Spire reads as one game.
- **Tiles are seamless.** All three kits measured clean in pixel diffs.
- **Edits stay local.** An agent could trace a problem in a scene to one line and change only that line.
- **Agents learn it from the docs alone.** With the skill and `artscript docs`, ten agents shipped usable art without reading the source.

### What holds it back

- **Silent wrong output.** These are the worst problems:
  - `rect w x h` draws a square (#55).
  - `fmt` rewrites a valid file into one that does not parse (#56).
  - Anchor reads inside a def apply their offset twice, so mirrored characters come out scrambled (#51).
  - `--only` with several globs builds less than asked and exits 0 (#65).
- **No engine-facing output.** There is no pivot or anchor export (#69) and no atlas (#70). Mirroring a sprite also mirrors its lighting (#76). Each part needs two files (#71). Several agents wrote Python generators to produce `.art` files.
- **Checks that cannot be trusted.** The off-canvas lint and `assert inside` ignore outlines and glows (#61), and they count geometry that a clip hides (#58). `check` cannot be limited to your own files (#72).
- **A mid quality ceiling.** All edges are soft vector edges. There is no pixel-grid mode (#82) and no real lighting (#77). The agents rated their own art as good placeholders, not final art.
- **Slow builds.** A 1080p scene took over 3 minutes on the CPU backend (#86). Part of this is our config, which forced the CPU backend.

These fixes would move it to an 8:

1. Fix the silent bugs.
2. Add pivot and anchor export and an atlas target.
3. Let one file be both a def and an output.

At an 8 we would trust it for UI, icons, props and tiles. Characters would still need hand polish.

## What a 10 needs

At a 10, the team makes every 2D asset in the game with artscript, by choice. It produces better art faster than Aseprite plus a painter. The work from 8 to 10 is not bug fixing. It is three things: an art ceiling at shipped quality, engine-native output, and a fast feedback loop the team can trust.

### 1. Raise the ceiling to final art

- **Pixel-art mode.** Snap to the pixel grid, turn off anti-aliasing, limit the palette and draw clean 1-pixel outlines (#82). Most metroidvanias are pixel art.
- **Lighting.** Add light sources that can brighten as well as darken. Let shared finishes read a light direction (#76, #77). Add optional normal-map output so the engine can relight sprites, which also fixes mirrored lighting at its root.
- **Materials.** Texture must tile across tile edges and keep its pinned seed (#73, #74). Glows must be stronger. Translucent fills must keep their outlines (#57). Ice, lava and moss should look like materials, not flat fills.
- **Character construction.** Add real joints, two-bone limbs, and foot placement on slopes. Our hero could not stand on a 45-degree ramp.

### 2. Speak the engine's language

- Write pivots, anchors, sockets, hitboxes and draw order to JSON next to each PNG (#69).
- Add atlas and tileset targets with autotile rules, exported to Godot, Unity or Tiled (#70, #74).
- Make rig assembly part of the language, and export to a skeletal format such as Spine or DragonBones (#80).
- Add 9-slice metadata for UI frames and per-size output settings (#78, #81).
- Make palette swaps first class, for elite enemies and biome variants (#75).

### 3. Make the loop fast and trustworthy

- No silent wrong output. Every lint and assert counts what is actually drawn (#58, #61).
- Rebuild in under a second, with a watch mode and a live preview (#86).
- Add scene-level asserts for art review. Examples are a 3:1 contrast floor between the hero and the background, feet on the floor line, and the sentinel at 1.6x the hero's height. Our agents measured all of these by hand in Python. As asserts, art review becomes a test suite.
- Show a visual diff between two builds.

### 4. Make a team faster than the alternatives

- One file per asset (#71), a path filter for `check` (#72), and lint that names the entry file that triggered it (#66).
- A shared library of rigs, tile families and FX, so the hundredth enemy is cheap.
- A round trip for human artists. A painter's touch-up must survive the next build, or land as a layer that the build composes.

### Order of work

| Step | Work | Rating after |
|---|---|---|
| 1 | Trust fixes: no silent bugs, honest checks | about 7 |
| 2 | Engine export: pivots, atlases, one file per asset | about 8 |
| 3 | Pixel mode and lighting | about 9 |
| 4 | Fidelity a player cannot tell from hand-made art | 10 |

The last step depends on fidelity more than on features, and it is the hardest. It is not proven that a declarative language can reach a hand-painted standard. This playtest stopped at "good placeholder".

Some items have no issue yet. They are watch mode, normal maps, skeletal export, scene-level art asserts and the human round trip.
