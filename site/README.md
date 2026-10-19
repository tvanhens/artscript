# The artscript landing page

A static page: `index.html`, `styles.css` and `main.js`. It has no build step. Open `index.html` in a browser, or serve this folder.

Every picture on the page is artscript output, built from the sources in this folder:

- `art/moths/` holds the moth family. `lib/moth.art` is the one component. `art/plate.art` builds the 16 specimens, `art/demo.art` builds the 40 images behind the demo controls, `art/specimen.art` builds the hero, and `art/flutter.art` builds the GIF.
- `art/studies/` holds the six feature studies and the sprout that the agent-loop section follows. `evidence/` holds the real `check --json` output, the one-line diff and the manifest hashes that the section shows.
- `brand/` holds the transparent nav mark. Its def is copied from `assets/logo.art`.

## Rebuild the art

Build the CLI from this checkout, then build each project from its own folder:

```sh
cargo build --release -p artscript
cd site/art/moths && ../../../target/release/artscript build
```

In `art/studies/`, build with `--only 'art/{ramp,clip,brush,stamp,field,sway,sprout}.art'`. `art/sprout_broken.art` fails its `assert` on purpose, so a full build there exits 2.

The page shows source text verbatim in `<pre>` blocks. When you change an `.art` file the page quotes, update its copy in `index.html`.
