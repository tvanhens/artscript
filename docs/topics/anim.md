# anim: t, key, anim, play, node keys, explain

`t` is the clip time in seconds. Inside an `anim` it is that anim's local time. `clip` is the target's `clip:` name, or `none`.

```
key(0 6 0)
key(0 1 1 0 at:[0 .3 .7 1] ease:in_out)
anim idle dur:2s loop
  .face at:0,key(0 6 0)
  .leaf rot:key(-4 4 -4)
anim blink dur:200ms
  .eye lid:key(.6 1 .6)
play idle start:0s
```

- `key` samples the current time. Values are spaced evenly unless `at:` lists fractions from 0 to 1. The default curve is smooth. `ease:` is `linear`, `in`, `out`, `in_out`, or `hold`, one name or one per segment. `loop` wraps. Outside an anim, `dur:` is required.
- Numbers, angles, vectors, and sizes interpolate. Colors interpolate in OKLab. Words, components, and booleans step. Anything else is an error.
- `anim` is a declaration. Its body is patch lines. `at` and `rot` add to the rest pose. A percent rest `at` is measured against the node's frame before a numeric offset is added. `scale` multiplies. `abs(v)` on those three sets the value. Every other argument is set, except `name:`, which stays on the draw. A patch expression is seeded from the anim's file, the owner key, the anim's name, and the patch's path, so a comment or a blank line keeps its value. A `style:` word from the anim is resolved in the anim's module. `play` is a statement at the top of an entry file or under an instance. `play none` does nothing. Plays apply in source order after static patches. A play under an instance in a `def` body applies in the order the instances are drawn, and an entry-file play that reaches the same node wins over it. A top-level `play` is scheduled before anything is drawn, so its `start:`, its anim's expressions, and an `if` or `each` around it cannot read geometry such as `@name.w` (E0344). The last set value wins. A `let` inside `if` or `each` under an instance stays in that branch. A direct `let` under the instance is visible to later plays there.
- A named node's key ignores layers and groups, so `layer body` holding `name:robe` is `robe`. An unnamed node's key is `verb~N` under its parent. `explain` prints the key, including nodes drawn in a mask block. A `~` means the node is not addressable. A node that drew a seed from its key is marked `seed`.
