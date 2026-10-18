# components: def, size box, slots, patches, let/if/each, style

```
def eye size:10 iris:p.sky.3 look:0,0
  circle size fill:#fff name:white
    circle size*.55 at:look*size*.3 fill:iris
  anchor brow 0,-size
  slot lashes at:0,-size*.8
```

- The first parameter is positional (`eye 9 at:-14,0`), and types come from the defaults.
- `size` is the def's box, centered on the origin. `size:40` is a square. `size:72x32` is 72 wide and 32 tall. A bare percent uses the smaller side. Percents in the body measure that box, and a child with no `at:` stays at the origin.
- Components are values, so `outfit:tunic` can be placed as `outfit`, and `none` draws nothing. A def may reuse a shape word. `def circle` is that component, not the built-in circle.
- `slot name` exposes an insertion point, filled by the caller with a `name:` block. A def body may hold `fx`, which attach to every instance and run before the call site's. A mask, a slot fill, or a patch cannot sit directly in a def body. Slot content is its own scope. `@part` there searches the content, then the host, then the caller. A def body sees only its own names. `needs:@torso` imports the host's `torso`, and a missing host or a missing part is an error at the call.
- `.arm_r rot:-20`, `.cape off`, and `.head.eye_l iris:p.accent` override a declared name from the instance block, in source order, with the last one winning. Every segment before the last is an instance or a slot, so `.head.eye_l` needs `head` to be one. `.eye.0` reaches one copy of a family, and `.eye.1 off` hides that copy. A segment may be a keyword when the part is a slot of that name, as in `.mask off`. Hiding the instance with `off` still checks a hop through it.
- A part whose name starts with `_`, as in `name:_core`, is private to its def. `@_core` works in the def body. A patch or an anim path from outside the def that names `_core` is an error, and `.*` skips it. The entry file has no caller, so its own `_` names stay patchable.
- `let w = if build == slim then 34 else 42`, `if`/`else` (a statement `if` takes a block, not `then`), and `each x in [..]` / `each i in 0..8` are bounded control flow. `each i in 0..8` runs 8 times, with `i` from 0 through 7. `[each k in 0..n -> expr]` builds the list.
- `style ink stroke:p.ink,2` with `style:ink` applies those keys to a node and its descendants. A style takes keys only, never a block. A style holds keys that children inherit, while a finish holds effects that one node gets.
