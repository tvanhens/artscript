# Enlarge the strawberry's face

kind: multi
piece: produce/sweet_strawberry

## Request

The face gets lost when the berry is small. Make the eyes, the blush, and the mouth about 10 to 15 percent larger. Keep the face centered, and keep the two eyes level.

## Do

In `art/produce/sweet_strawberry.art`, enlarge these and leave their positions alone.

On `def _eye x:436 side:-1 lid:.18 w:72 h:86`, set `w:` to 81 (between 80 and 83) and `h:` to 97 (between 95 and 99). Both open eyes, `_eye name:eye_l` and `_eye 588 side:1 name:eye_r`, take those defaults.

On both closed eyes, `_eye lid:1 w:70 h:84` and `_eye 588 side:1 lid:1 w:70 h:84`, scale by that same factor. Set `w:` to 79 (between 78 and 81) and `h:` to 95 (between 93 and 97).

On the blush pair inside `each x in [378 646]`, change `blush 86x40` to `97x45` (width between 95 and 99, height between 44 and 46) and `blush 56x24` to `63x27` (width between 62 and 64, height between 26 and 28). Keep `at:x,648` and the x values `378` and `646`.

On `open_mouth at:512,642 color:berry.lip`, set an explicit size of 34 (between 33 and 35). The default is 30. Keep `at:512,642`.

The existing `assert level(@eye_l @eye_r) tol:.5` must still pass.

## Acceptance

```accept
changed produce/sweet_strawberry.png 317,554 390x130
assert art/produce/sweet_strawberry.art @eye_l.w in 84..92
assert art/produce/sweet_strawberry.art @eye_r.w in 84..92
assert art/produce/sweet_strawberry.art @eye_l.h in 82..92
assert art/produce/sweet_strawberry.art @open_mouth.w in 34..40
```
