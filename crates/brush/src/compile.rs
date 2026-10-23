//! Brush shaders compiled with Skia: the check eval runs at load and the effects render draws
//! with.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use skia_safe::RuntimeEffect;

use crate::{Frame, MainKind, Shader, sksl};

/// Compiles every (main, frame) pair once. The checker accepted the body, so an `Err` is a bug
/// in the SkSL backend; eval reports it once at the `brush` line. Warms this thread's memo.
pub fn verify(shader: &Shader) -> Result<(), String> {
    for main in [MainKind::Area, MainKind::Ribbon] {
        for frame in [Frame::Shader, Frame::Blender] {
            effect(shader, main, frame)?;
        }
    }
    Ok(())
}

/// The printed body and the main and frame it was assembled under.
type Key = (Arc<str>, MainKind, Frame);

thread_local! {
    // `RuntimeEffect` is neither `Send` nor `Sync` in skia-safe, and `Sendable` wraps only a
    // uniquely owned handle, so a shared memo cannot hand out clones. One memo per thread.
    static EFFECTS: RefCell<HashMap<Key, RuntimeEffect>> = RefCell::new(HashMap::new());
}

/// The compiled effect for `shader` under `main` and `frame`, memoized per thread by the printed
/// body. The frames are assembled only on a miss.
pub fn effect(shader: &Shader, main: MainKind, frame: Frame) -> Result<RuntimeEffect, String> {
    let key = (shader.sksl().clone(), main, frame);
    let hit = EFFECTS.with(|m| m.borrow().get(&key).cloned());
    if let Some(e) = hit {
        return Ok(e);
    }
    let text = sksl::assemble(shader.sksl(), main, frame);
    let made = match frame {
        Frame::Shader => RuntimeEffect::make_for_shader(text, None),
        Frame::Blender => RuntimeEffect::make_for_blender(text, None),
    };
    let e = made.map_err(|e| format!("the SkSL backend rejected a checked body: {e}"))?;
    EFFECTS.with(|m| m.borrow_mut().insert(key, e.clone()));
    Ok(e)
}
