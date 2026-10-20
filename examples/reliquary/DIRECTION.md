# The Reliquary of the Last Sun

A side-view room in a drowned nave. One caged sun, upper right, is the only key light. The water gives back a cool rim. Bone and void are most of the picture. Amber is scarce. Rose is only the far window and the page's cloak.

This file is the contract. Do not edit it. Do not edit `ramose.toml`, `lib/`, `art/scene/`, `art/study/`, or any file you do not own.

## What the picture is

The player is a small lantern page who has just stepped into the nave. A censer-headed warden stands in the water. An eel lies in the pool. A salt pilgrim waits on a ledge. Above, glass moths sing from the vault, and a resin hart stands under the cage that holds the sun. The goal of the room is that cage.

## Light

`sun:.62,-.84` on `sculpt` and `ink` already points at the cage. Do not pass a different `sun`. Highlights sit on the upper right of a form. Shadow sits on the lower left. The cool rim is `p.rim`, and it belongs on the lower left edge, which the brush already paints. A second key is a broken picture.

Emissive things (the sun, a flame, the eel's lure, the page's lantern, resin) may glow amber. They do not re-light the room.

## Palette

Paint with `p` only. `raw-color` is an error.

- `bone` stone, ribs, salt, the cage
- `verd` old copper and plate
- `tide` and `pool` water
- `amber` the sun and anything that burns
- `flesh` pale creatures
- `glass` wings
- `rose` the window and the cloak only
- `ash` smoke
- `moss` a little growth in cracks
- `ink`, `shine`, `rim`, `void`

Most of a sprite is bone, flesh, or verd. Amber is a small hot note.

## How to draw

`assets/study/marks.png` is the material. The left slab is the stone. The right cluster is the amber, the bone rib, and the verdigris, seen at a size where the brush reads. It is not a composition to copy.

`examples/spire/assets/monster/bogmaw_card.png` is the density. Match that many overlapping forms, that much surface incident, and a silhouette that would read if you filled it flat. Do not copy the bogmaw, its palette, or its brushes.

Recipe:

- Big forms first, then the parts that overlap them, then cracks, pits, crust, and one small thing a second look would find.
- Nest highlights and shadow shapes under the form they belong to. A child is clipped to its parent.
- Stamps are not clipped. Put `stamp:crust` and `stamp:ember` only where the spill is wanted.
- `brush:sculpt` on every painted fill or thick stroke. `fx outline` with `brush:ink` on a main form, wider on the shadow side because the brush does that. No even sticker outline.
- `contact` under anything that stands on the ground, inside the def, at the feet.
- One `eye` from `lib/parts.ramo` when a creature has eyes. The catchlight is already upper right.
- Name every node that uses `rand`. An unnamed one is an error.
- A layer name and a shape name do not share a scope. Name layers `body_layer`, not `body`, when a shape is also `body`.
- `bg:none` on the `@target`. The room supplies the dark.

## Anchor

The def origin is the anchor. Draw the body around it.

- Feet. The soles sit on y = 0. The body grows up, which is negative y. `assert @name.bottom in -8..16`.
- Hang. The hook sits on y = 0. The body grows down, positive y. `assert @name.top in -16..8`.
- Center. The origin is the visual center. Keep the whole sprite inside the canvas with `assert inside(@name canvas margin:12)`.

Glow and blur count toward the canvas edge. Grow the canvas before you clip a glow.

## Owned files

Each file is one entry. It declares one `def` and draws that def once, named, with the asserts above.

| File | Def | Anchor | Canvas | What it must show |
|---|---|---|---|---|
| `art/bg/rose_window.ramo` | `rose_window` | center | 560x680 | A distant rose window of bone tracery and rose glass, a sun being swallowed by ribs. Lower contrast than a foreground prop. |
| `art/bg/vault.ramo` | `vault_ribs` | center | 1440x380 | A run of bone vault ribs across the ceiling, seen from the side, receding. Darker at the left, warmer at the right. |
| `art/arch/colonnade.ramo` | `colonnade` | feet | 340x680 | Two broken bone columns and a stump of arch, salt on the broken tops, one verdigris ring. |
| `art/arch/stair.ramo` | `vertebra_stair` | feet | 480x540 | A stair of vertebrae climbing up and to the right. Each step is a bone. The top step is chipped. |
| `art/arch/sun_cage.ramo` | `sun_cage` | center | 440x500 | The focal object. A ribcage of bone closed around a blazing amber sun. Thicker ribs in front. Smoke. This is the key light made visible. |
| `art/ground/nave.ramo` | `nave_floor` | hang | 1440x260 | The floor of the nave. y = 0 is the top, the waterline. Flagstones, a darker pool in the middle, salt, a reflection of amber toward the right. |
| `art/ground/ledge.ramo` | `salt_ledge` | hang | 400x120 | One platform. y = 0 is the top surface. Front face, chipped corner, salt crust, a dark underside. It will be placed more than once, so it must read alone. |
| `art/prop/offering.ramo` | `offering_bowl` | feet | 240x180 | A verdigris bowl of ash with one live ember. |
| `art/prop/crown.ramo` | `fallen_crown` | feet | 260x150 | A bone crown with amber inlays, lying on its side, one tine snapped. |
| `art/prop/candles.ramo` | `anemone_candles` | feet | 260x240 | A cluster of glass anemones used as candles. Petals of `glass`, flames of `amber`. |
| `art/flora/root.ramo` | `copper_root` | hang | 200x480 | A hanging copper root with verdigris leaves and one glass bud. It occludes whatever is behind it. |
| `art/monster/warden.ramo` | `censer_warden` | feet | 440x580 | The room's knight. Side view, facing right. No human face. The head is a censer of bone ribs around an amber heart, with a chain. Verdigris plate, one arm a hanging chain, two legs in the water. Smoke. |
| `art/monster/choir.ramo` | `glass_choir` | hang | 380x320 | Three moth-bodied singers hanging from one stem. Stained-glass wings, small open mouths, no two the same pose. |
| `art/monster/eel.ramo` | `marrow_eel` | feet | 440x260 | A pale segmented eel coiled so the belly sits on y = 0. A lantern lure on a whisker. Wet highlights. |
| `art/monster/pilgrim.ramo` | `salt_pilgrim` | feet | 180x240 | A small crusted pilgrim, shell on the back, staff of bone, facing right. Readable at half size. |
| `art/monster/hart.ramo` | `resin_hart` | feet | 480x500 | A deer skull on a thin body, antlers of amber resin that branch like a window. Still, facing right, larger than the warden. |
| `art/hero/page.ramo` | `lantern_page` | feet | 160x220 | The player. Small. A square lantern, a rose cloak, a hood, facing right. The silhouette reads at 90 pixels tall. |
| `art/fg/pier.ramo` | `cracked_pier` | feet | 280x760 | A foreground column, darker and larger than the colonnade, cracked, cut off by the frame of its own canvas at the top. It will occlude the room. |
| `art/fx/embers.ramo` | `ember_field` | center | 480x360 | Loose embers and ash on a transparent ground. No solid body. Sparse. For scattering in the light shaft. |

## Loop

From `examples/reliquary`, with `ramose` on `PATH` (`/workspace/target/debug`):

1. Write only your file.
2. `ramose check <your-file>` and `ramose build --only '<your-file>' --backend cpu`.
3. Open the PNG and look. Fix what you see. Do this at least three times. The third look is silhouette, light direction, and one incident.
4. `ramose fmt <your-file>`.
5. Do not commit. Do not format the rest of the project.

Report `PASS`, `ISSUES`, or `BLOCKED`, the output path, and what you saw in the PNG on the last look.
