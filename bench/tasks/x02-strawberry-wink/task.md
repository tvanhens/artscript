# Give the strawberry a wink

kind: variant
piece: produce/sweet_strawberry
reads: lib/face/parts.art
looks: produce/sweet_strawberry_wink

## Request

The strawberry needs a wink. Close the eye on the left as you look at the berry, leave the other eye open, and turn the open mouth into a smaller smile. Leave the plain portrait as it is.

## Do

In `art/produce/sweet_strawberry.art`, add `@target produce/sweet_strawberry_wink.png 1024x1024 as:wink` beside the svg target. Import `smile` from `face/parts` next to `open_mouth`.

On `layer eyes`, the left eye (`_eye name:eye_l`) gets `a:` 0 when `target == wink` and 1 otherwise. Leave the right eye (`_eye 588 side:1 name:eye_r`) open on every target.

On `layer eyes_closed` (the line that starts `layer eyes_closed pivot:512,592 a:0`), set `a:` to 1 when `target == wink` and to 0 otherwise. On its right eye (`_eye 588 side:1 lid:1 w:70 h:84`), set `a:` to 0 when `target == wink` and to 1 otherwise, so only the left eye closes. Leave the left closed eye (`_eye lid:1 w:70 h:84`) unchanged.

In `layer mouth`, when `target == wink` draw `smile` with `color:berry.lip`, size 18 to 30, and `at:512` with y from 642 to 658. Otherwise keep `open_mouth at:512,642 color:berry.lip`.

## Acceptance

```accept
unchanged produce/sweet_strawberry.png 0,0 1024x1024
differs produce/sweet_strawberry_wink.png produce/sweet_strawberry.png 388,554 87x86
differs produce/sweet_strawberry_wink.png produce/sweet_strawberry.png 492,637 40x27
```
