# Debug an animated attachment

This example draws a blue arm with an orange hand. The hand must touch the arm throughout its swing. All commands run from this directory with a CLI built from this checkout.

1. Read the matching documentation, build the normal artwork, and open `assets/arm.png` at 128 by 128 pixels with your host's image viewer.

   ```sh
   ramose docs cli values components targets anim craft
   ramose check
   ramose build
   ```

2. To reproduce an attachment defect, change `let attachment = 0` to `let attachment = 8` in `art/arm.ramo`. Rebuild and open the normal output. The hand separates from the arm. Enable geometry mode and construction lines to locate the intended joint.

   ```sh
   ramose check art/arm.ramo --set view=geometry --set guides=true
   ramose preview art/arm.ramo --target arm.png --set view=geometry --set guides=true --crop hand --pad 14 --zoom 4 --bg checker --out review/joint-debug.png
   ```

   Open `review/joint-debug.png`. The cyan crossing marks the arm endpoint. This is a parameter-altered view, not the shipping image. The JSON receipt records the resolved parameters and crop.

3. Inspect the normal named crop against each presentation background. Open all three PNGs. The crop preserves the arm around the hand and enlarges the original target pixels.

   ```sh
   ramose preview art/arm.ramo --target arm.png --crop hand --pad 14 --zoom 4 --bg light --out review/hand-light.png
   ramose preview art/arm.ramo --target arm.png --crop hand --pad 14 --zoom 4 --bg dark --out review/hand-dark.png
   ramose preview art/arm.ramo --target arm.png --crop hand --pad 14 --zoom 4 --bg checker --out review/hand-checker.png
   ```

4. Inspect the animation for attachment drift, clipping, and a loop seam. Open both generated sheets. Compare the first and last cells of the explicit strip. They sample the same loop pose. The evenly sampled sheet excludes the repeated endpoint.

   ```sh
   ramose preview art/arm.ramo --target swing.gif --frames 8 --columns 4 --out review/swing-sheet.png
   ramose preview art/arm.ramo --target swing.gif --times 0,.5,1,1.5,2 --out review/swing-seam.png
   ```

5. Restore `let attachment = 0`, then check and build without overrides. Open `assets/arm.png` again at its intended size and regenerate the normal animation sheets. Confirm the hand touches the arm in every sampled pose and neither part clips the canvas.

   ```sh
   ramose check
   ramose build
   ramose preview art/arm.ramo --target swing.gif --times 0,.5,1,1.5,2 --out review/swing-fixed.png
   ```

Report the normal output, diagnostic paths, and parameter settings separately. Preview files stay in `review/`. No temporary targets or external image-processing scripts are needed.
