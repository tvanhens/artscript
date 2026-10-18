# manifest: artscript.toml tables and keys, project defaults, lint levels

`artscript.toml` marks the project root. `artscript init` writes this one:

```
[project]
name = "art"
out  = "assets"
lib  = ["lib"]
std  = "0.1"

[defaults]
palette = "palette/house"
scale   = [1]
bg      = "none"
profile = "auto"

[lint]
off-canvas = "off"
```

Every table and key is optional, and a missing key takes the default named below. An unknown table or key, a lint name that is not in the list below, and a value of the wrong type are each E0200 at the line that has the problem. Nothing builds until the manifest is fixed.

`[project]`:

- `out` is the output directory, relative to the root. The default is `assets`. `build` and `check` do not read `.art` files inside it.
- `lib` is one lib root or a list of them. The default is `["lib"]`. `use face/eye` reads `face/eye.art` from the first lib root that has it.
- `name` and `std` are strings. The build does not read them.

`[defaults]` sets the value for each target that leaves out the matching `@target` key (see `targets`):

- `palette` is a library path such as `palette/house`. Every file reads that palette as `p`, unless a `palette:` key on a target or an instance swaps it. A palette that no lib root holds is E0200. With no `palette` key, `p` is not defined.
- `scale` is a positive number or a list of them. The default is `[1]`.
- `bg` is `"none"` or a hex color such as `"#f4f1ea"`. The default is `"none"`.
- `profile` is `vector`, `svg`, `raster`, or `auto`. The default is `auto`.

`[build]`:

- `backend` is `auto`, `gpu`, or `cpu`. The default is `auto`. `build --backend` overrides it.

`[lint]` sets the level of each lint:

| name | code | reports |
|---|---|---|
| `raw-color` | L0101 | a hex or `oklch(…)` color outside a palette |
| `off-canvas` | L0102 | a node that the canvas edge cuts (see `checks`) |
| `unused` | L0103 | an imported or private name that nothing reads (see `targets`) |
| `positional-seed` | L0104 | an unnamed statement whose randomness comes from its position (see `fields`) |
| `hidden-child` | L0105 | a line nested under a shape that draws nothing there (see `checks`) |

Each lint is `warn` unless the table sets it. The levels are `off`, `info`, `warn`, and `error`. `off` drops the lint. `info` prints it and keeps exit code 0. `warn` makes `build` and `check` exit 3, unless `build --allow-warnings` is set. `error` makes them exit 2. A compile error still exits 1. A scene that draws past the canvas edge on purpose marks its target `@target <path> <W>x<H> viewport` (see `targets`); `off-canvas = "off"` turns the lint off for every target.
