//! Brush programs compiled with Skia: the check eval runs and the effects render draws with.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use skia_safe::RuntimeEffect;

use crate::{MainKind, Origin, Program, SkslError, UniformKind};

/// Compiles the shader frame once to validate. Every error Skia lists comes back, each mapped
/// to its origin.
pub fn check(program: &Program) -> Result<(), Vec<SkslError>> {
    match RuntimeEffect::make_for_shader(program.source().as_ref(), None) {
        Ok(_) => Ok(()),
        Err(text) => Err(parse_errors(program, &text)),
    }
}

/// [`check`] for the blender frame, which a body runs under when any stage of its stack reads
/// `m.below`.
pub fn check_blender(program: &Program) -> Result<(), Vec<SkslError>> {
    match RuntimeEffect::make_for_blender(program.blend_source().as_ref(), None) {
        Ok(_) => Ok(()),
        Err(text) => Err(parse_errors(program, &text)),
    }
}

/// SkSL's message when `uniform <kind> <name>;` does not compile on its own, as when `name` is a
/// keyword, a type or a builtin. The compiler is the authority, so no word list can drift from it.
pub fn rejects_param_name(name: &str, kind: UniformKind) -> Option<String> {
    let probe = Program::assemble(MainKind::Area, &[(name.to_string(), kind)], "float4 shade(Mark m) { return m.paint; }");
    let errs = check(&probe).err()?;
    errs.into_iter().find(|e| e.origin == Origin::Param(0)).map(|e| e.message)
}

/// Skia prints each error as `error: <line>: <message>`, then the source line, then a line of
/// spaces and carets under the span. It ends with `N errors`. A message with no line, such as
/// `missing 'main' function`, stands alone.
fn parse_errors(program: &Program, text: &str) -> Vec<SkslError> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        i += 1;
        let Some(rest) = line.strip_prefix("error: ") else {
            if !line.trim().is_empty() && !is_count(line) {
                out.push(SkslError {
                    origin: Origin::Frame,
                    message: line.trim().to_string(),
                });
            }
            continue;
        };
        let (num, message) = match rest.split_once(": ") {
            Some((n, m)) if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) => {
                (n.parse::<u32>().ok(), m.to_string())
            }
            _ => (None, rest.to_string()),
        };
        let mut col = None;
        if num.is_some() && i + 1 < lines.len() {
            let caret = lines[i + 1];
            if !caret.is_empty() && caret.trim_start_matches(' ').chars().all(|c| c == '^') {
                let shown = caret.chars().take_while(|c| *c == ' ').count();
                let source = num.and_then(|n| program.source().lines().nth(n as usize - 1)).unwrap_or("");
                col = Some(source_col(source, shown));
                i += 2;
            }
        }
        out.push(SkslError {
            origin: program.origin_of(num, col),
            message,
        });
    }
    if out.is_empty() {
        out.push(SkslError {
            origin: Origin::Frame,
            message: text.trim().to_string(),
        });
    }
    // Errors past the first in the frame name its internals (`o`, `rgb`) and follow from it.
    let mut frame_seen = false;
    out.retain(|e| e.origin != Origin::Frame || !std::mem::replace(&mut frame_seen, true));
    out
}

/// The character index in `line` under Skia's caret column. Skia echoes each tab as 4 spaces.
fn source_col(line: &str, shown: usize) -> u32 {
    let mut width = 0;
    for (i, c) in line.chars().enumerate() {
        if width >= shown {
            return i as u32;
        }
        width += if c == '\t' { 4 } else { 1 };
    }
    line.chars().count() as u32
}

fn is_count(line: &str) -> bool {
    let line = line.trim();
    line.ends_with(" error") || line.ends_with(" errors")
}

thread_local! {
    // `RuntimeEffect` is neither `Send` nor `Sync` in skia-safe, and `Sendable` wraps only a
    // uniquely owned handle, so a shared memo cannot hand out clones. One memo per thread.
    static EFFECTS: RefCell<HashMap<Arc<str>, RuntimeEffect>> = RefCell::new(HashMap::new());
}

/// The compiled shader frame, memoized per thread by source text. `check` accepted this same
/// text, so a failure here is an internal error and the message says so.
pub fn effect(program: &Program) -> Result<RuntimeEffect, String> {
    compiled(program.source(), |text| RuntimeEffect::make_for_shader(text, None))
}

/// The compiled blender frame, memoized like [`effect`].
pub fn blender(program: &Program) -> Result<RuntimeEffect, String> {
    compiled(program.blend_source(), |text| RuntimeEffect::make_for_blender(text, None))
}

fn compiled(text: &Arc<str>, make: impl FnOnce(&str) -> Result<RuntimeEffect, String>) -> Result<RuntimeEffect, String> {
    let hit = EFFECTS.with(|m| m.borrow().get(text).cloned());
    if let Some(e) = hit {
        return Ok(e);
    }
    let e = make(text).map_err(|e| format!("brush program checked by eval failed to compile in render: {e}"))?;
    EFFECTS.with(|m| m.borrow_mut().insert(text.clone(), e.clone()));
    Ok(e)
}
