//! The brush shading contract, in one place: the SkSL prelude every brush body is compiled
//! with, the `Mark` struct and helpers an author may use, source assembly with a line map back
//! to the author's body, compile checking, the compiled-effect memo, and the seed reduction.
//!
//! Eval validates a brush declaration through [`check`] and render runs it through [`effect`].
//! Both go through [`Program::assemble`], so the text they see is identical.
//!
//! The hash, noise, `psin`, dither and 8-bit snap in the prelude are the CPU/GPU parity
//! contract: every intermediate of the lattice hash is an integer below 2^24, remainders are
//! divide-free, trig never calls the GPU's fast `sin`, and the output is snapped to 8-bit levels
//! after an in-shader dither so the two backends agree on whole levels. Do not edit those
//! functions without rerunning the parity comparison.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use skia_safe::RuntimeEffect;

/// Uniforms the runtime binds, `struct Mark`, and the helpers. Author bodies follow this text.
const PRELUDE: &str = r#"
uniform shader u_prev;
uniform shader u_sdf;
uniform float3x3 u_dtl;
uniform float4 u_paint;
uniform float u_base;
uniform float u_last;
uniform float u_seed;
uniform float2 u_seedo;
uniform float u_px;
uniform float2 u_sdfo;
uniform float4 u_frame;
uniform float3 u_dir;
uniform shader u_field;
uniform shader u_chan;
uniform float2 u_fieldo;
uniform float2 u_rib;
uniform float u_s0;

struct Mark {
    float2 p; float2 dev; float u; float s; float v; float d; float len; float width;
    float pressure; float angle; float tilt; float velocity;
    float edge; float2 uv; float seed; float4 paint; float px;
};

// Exact hashes: the input is quantized to 1/16 and every intermediate is an integer below
// 2.62e6 (1e5 in mod289). A remainder by multiplying with a rounded reciprocal plus half a step stays exact
// in that range under any rounding mode or FMA, and costs no divide or branch.
float modh(float x) { return x - 1613 * floor(x * 0.000619962802 + 0.000309981401); }
float3 mod289(float3 x) { return x - 289 * floor(x * 0.00346020761 + 0.00173010381); }
float hcore(float2 q) {
    float a = modh(q.x); float b = modh(q.y);
    float t = modh(a * 1597 + b);
    t = modh(t * t + a + 11);
    t = modh(t * t + b + 7);
    t = modh(t * t + a * 3 + 1);
    return t / 1613;
}
float hash12(float2 p) { return hcore(floor(p * 16 + 0.5)); }
float2 hash22(float2 p) {
    float2 q = floor(p * 16 + 0.5);
    return float2(hcore(q), hcore(q + float2(1373, 2741)));
}
float vnoise(float2 p) {
    float2 i = floor(p); float2 f = fract(p); float2 w = f * f * (3 - 2 * f);
    return mix(mix(hash12(i), hash12(i + float2(1, 0)), w.x),
               mix(hash12(i + float2(0, 1)), hash12(i + float2(1, 1)), w.x), w.y);
}
// x*34+1 and the product are reduced separately so every remainder input stays below 1e5.
float3 permute3(float3 x) { x = mod289(x); return mod289(mod289(x * 34 + 1) * x); }
float simplex(float2 v) {
    const float4 C = float4(0.211324865405187, 0.366025403784439, -0.577350269189626, 0.024390243902439);
    float2 i = floor(v + dot(v, C.yy));
    float2 x0 = v - i + dot(i, C.xx);
    float2 i1 = (x0.x > x0.y) ? float2(1.0, 0.0) : float2(0.0, 1.0);
    float4 x12 = x0.xyxy + C.xxzz;
    x12.xy -= i1;
    i = mod289(i.xyy).xy;
    float3 p = permute3(permute3(i.y + float3(0.0, i1.y, 1.0)) + i.x + float3(0.0, i1.x, 1.0));
    float3 m = max(0.5 - float3(dot(x0, x0), dot(x12.xy, x12.xy), dot(x12.zw, x12.zw)), 0.0);
    m = m * m; m = m * m;
    float3 x = 2.0 * fract(p * C.www) - 1.0;
    float3 h = abs(x) - 0.5;
    float3 ox = floor(x + 0.5);
    float3 a0 = x - ox;
    m *= 1.79284291400159 - 0.85373472095314 * (a0 * a0 + h * h);
    float3 g;
    g.x = a0.x * x0.x + h.x * x0.y;
    g.yz = a0.yz * x12.xz + h.yz * x12.yw;
    return 0.5 + 65.0 * dot(m, g);
}
float worley(float2 p) {
    float2 i = floor(p); float2 f = fract(p); float d = 8.0;
    for (int y = -1; y <= 1; y++) {
        for (int x = -1; x <= 1; x++) {
            float2 o = float2(float(x), float(y));
            float2 r = o + hash22(i + o) - f;
            d = min(d, dot(r, r));
        }
    }
    return sqrt(d);
}
float fbm(float2 p, float oct) {
    float s = 0; float a = .5; float n = 0;
    for (int i = 0; i < 8; i++) {
        if (float(i) >= oct) { break; }
        s += a * simplex(p); n += a; p = p * 2.03 + float2(17.1, 9.2); a *= .5;
    }
    return s / n;
}
// Polynomial sine on a reduced angle, so the GPU's fast sin/cos is never used.
float psin(float turns) {
    float t = turns - floor(turns + 0.5);
    t = t > 0.25 ? 0.5 - t : (t < -0.25 ? -0.5 - t : t);
    float x = t * 6.283185307; float x2 = x * x;
    return x * (1 + x2 * (-1.0 / 6 + x2 * (1.0 / 120 + x2 * (-1.0 / 5040 + x2 * (1.0 / 362880 - x2 / 39916800)))));
}
float2 rot(float2 p, float deg) {
    float tr = deg * (1.0 / 360); float c = psin(tr + 0.25); float s = psin(tr);
    return float2(c * p.x - s * p.y, s * p.x + c * p.y);
}
float2 seeded(Mark m) { return u_seedo; }
// Signed distance (local units, + inside) to coverage, antialiased over one pixel.
float cover(float d, Mark m) { return saturate(d / m.px + 0.5); }
"#;

/// The tail every main ends in, after the author's body so it can call `shade`. `u_base` set
/// reads the base paint (or the previous stage) from `u_prev`, snapped to 8-bit levels first so a
/// half-float child read gives the same levels as a float one. The output is premultiplied,
/// dithered on the last stage only, and snapped to 8-bit levels.
const FINISH: &str = r#"
half4 u_finish(Mark m, float2 c) {
    m.seed = u_seed; m.px = u_px;
    m.paint = u_paint;
    if (u_base > 0.5) {
        float4 pv = floor(float4(u_prev.eval(c)) * 255 + 0.5);
        m.paint = float4(pv.a > 0 ? pv.rgb / pv.a : float3(0), pv.a / 255);
    }
    float4 o = shade(m);
    float k = saturate(o.a) * m.paint.a;
    float3 rgb = o.rgb;
    if (u_last > 0.5) {
        // Exact per-pixel dither: Skia's CPU and GPU dithers differ, this one is bit-identical.
        rgb += (hcore(floor(c)) - 0.5) / 255;
    }
    // Snap to 8-bit levels so the GPU's half-float color path cannot round across a tie.
    float4 lv = floor(float4(saturate(rgb) * k, k) * 255 + 0.5);
    lv /= 255;
    return half4(lv);
}
"#;

/// The frame around an area body. The signed distance comes from a device-aligned field, one
/// texel per device pixel, so `u_sdf.eval(c - u_sdfo)` is an exact texel read on both backends.
const AREA_MAIN: &str = r#"
half4 main(float2 c) {
    float2 p = (u_dtl * float3(c, 1)).xy;
    float e = u_sdf.eval(c - u_sdfo).r;
    Mark m;
    m.dev = c; m.p = p;
    float2 q0 = p - u_frame.xy;
    float2 r = float2(u_dir.y * q0.x + u_dir.z * q0.y, -u_dir.z * q0.x + u_dir.y * q0.y);
    m.uv = r; m.s = r.x + u_frame.z * .5; m.d = r.y;
    m.len = u_frame.z; m.width = u_frame.w; m.u = m.s / m.len; m.v = r.y / (m.width * .5);
    m.pressure = 1; m.angle = u_dir.x; m.tilt = 0; m.velocity = 1;
    m.edge = e * u_px;
    return u_finish(m, c);
}
"#;

/// The frame around a ribbon body. `u_field` is a device-aligned field, one texel per device
/// pixel: arc length `s` to the spine in two half-float channels (whole chunks of [`S_CHUNK`]
/// device pixels past `u_s0` chunks, then the rest), the signed offset `d` across it, and the
/// half width there, all in device pixels. A negative rest marks a pixel past the ribbon's reach.
/// `u_chan` holds pressure, angle, tilt and velocity at the same texel. `u_rib` is the contour's
/// length in local units and 1 when it is closed. Past the end of an open contour `s` keeps
/// running, so `edge` rounds the cap.
const RIBBON_MAIN: &str = r#"
half4 main(float2 c) {
    float4 g = u_field.eval(c - u_fieldo);
    if (g.g < 0) { return half4(0); }
    float4 ch = u_chan.eval(c - u_fieldo);
    Mark m;
    m.dev = c; m.p = (u_dtl * float3(c, 1)).xy;
    m.s = ((g.r + u_s0) * 32 + g.g) * u_px; m.d = g.b * u_px;
    float hw = max(g.a * u_px, 1e-6);
    m.len = u_rib.x; m.u = m.s / m.len; m.width = 2 * hw; m.v = m.d / hw;
    m.pressure = ch.x; m.angle = ch.y; m.tilt = ch.z; m.velocity = ch.w;
    float over = u_rib.y > 0.5 ? 0 : (m.s < 0 ? m.s : max(m.s - m.len, 0));
    m.edge = hw - length(float2(over, m.d));
    m.uv = float2(m.s, m.d);
    return u_finish(m, c);
}
"#;

/// Device pixels per whole-chunk step of the ribbon field's `s` channel. The rest channel then
/// stays below this, where a half float resolves 1/64 of a pixel.
pub const S_CHUNK: f32 = 32.0;

/// Fields of `struct Mark`, in declaration order. A docs test asserts `docs/topics/brushes.md`
/// names every one.
pub const MARK_FIELDS: &[&str] = &[
    "p", "dev", "u", "s", "v", "d", "len", "width", "pressure", "angle", "tilt", "velocity",
    "edge", "uv", "seed", "paint", "px",
];

/// Functions an author body may call. Same docs test.
pub const HELPERS: &[&str] =
    &["hash12", "hash22", "vnoise", "simplex", "worley", "fbm", "rot", "seeded", "cover"];

/// Identifier prefix the runtime owns. A body or a param that uses it is rejected by eval.
pub const RESERVED_PREFIX: &str = "u_";

/// Placement keys a brush declaration or call may set. Every other head key is a uniform.
pub const PLACEMENT_KEYS: &[&str] = &["dir", "reach", "on"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MainKind {
    Area,
    Ribbon,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UniformKind {
    Float,
    Float2,
    /// Straight rgba.
    Float4,
}

impl UniformKind {
    pub fn sksl(self) -> &'static str {
        match self {
            UniformKind::Float => "float",
            UniformKind::Float2 => "float2",
            UniformKind::Float4 => "float4",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UniformValue {
    Float(f32),
    Float2([f32; 2]),
    Float4([f32; 4]),
}

impl UniformValue {
    pub fn kind(&self) -> UniformKind {
        match self {
            UniformValue::Float(_) => UniformKind::Float,
            UniformValue::Float2(_) => UniformKind::Float2,
            UniformValue::Float4(_) => UniformKind::Float4,
        }
    }

    pub fn floats(&self) -> &[f32] {
        match self {
            UniformValue::Float(v) => std::slice::from_ref(v),
            UniformValue::Float2(v) => v,
            UniformValue::Float4(v) => v,
        }
    }
}

/// Assembled source: prelude, one `uniform` line per param, the author's body, the shared finish,
/// the kind's main. Equality and hashing are by text, which is also the compile memo key.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Program {
    source: Arc<str>,
    /// Assembled line of the first `uniform` param line; params follow one per line.
    params_at: u32,
    params: u32,
    /// Assembled line of body line 1.
    body_at: u32,
    body_lines: u32,
}

/// Where an assembled-source line came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// 1-based line in the author's body, and a 0-based column in that line when Skia gave one.
    Body { line: u32, col: Option<u32> },
    /// The `i`th declared param's `uniform` line.
    Param(usize),
    /// The prelude or the main, or no line at all: the body as a whole is at fault.
    Frame,
}

impl Program {
    /// Deterministic: the same inputs give the same text. Params are emitted in the given order.
    pub fn assemble(kind: MainKind, params: &[(String, UniformKind)], body: &str) -> Program {
        let mut src = String::from(PRELUDE);
        let params_at = lines_before_next(&src);
        for (name, kind) in params {
            src.push_str(&format!("uniform {} {name};\n", kind.sksl()));
        }
        let body_at = lines_before_next(&src);
        src.push_str(body);
        if !body.ends_with('\n') {
            src.push('\n');
        }
        let body_lines = body.split('\n').count() as u32;
        src.push_str(FINISH);
        src.push_str(match kind {
            MainKind::Area => AREA_MAIN,
            MainKind::Ribbon => RIBBON_MAIN,
        });
        Program {
            source: src.into(),
            params_at,
            params: params.len() as u32,
            body_at,
            body_lines,
        }
    }

    pub fn source(&self) -> &Arc<str> {
        &self.source
    }

    /// Maps a 1-based assembled line and Skia's caret column to the author's text.
    pub fn origin_of(&self, line: Option<u32>, col: Option<u32>) -> Origin {
        let Some(line) = line else { return Origin::Frame };
        if line >= self.body_at && line < self.body_at + self.body_lines {
            return Origin::Body { line: line - self.body_at + 1, col };
        }
        if line >= self.params_at && line < self.params_at + self.params {
            return Origin::Param((line - self.params_at) as usize);
        }
        Origin::Frame
    }
}

/// The 1-based line number the next appended character lands on.
fn lines_before_next(src: &str) -> u32 {
    src.matches('\n').count() as u32 + 1
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkslError {
    pub origin: Origin,
    pub message: String,
}

/// Compiles once to validate. Every error Skia lists comes back, each mapped to its origin.
pub fn check(program: &Program) -> Result<(), Vec<SkslError>> {
    match RuntimeEffect::make_for_shader(program.source().as_ref(), None) {
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

/// The compiled effect, memoized per thread by source text. `check` accepted this same text,
/// so a failure here is an internal error and the message says so.
pub fn effect(program: &Program) -> Result<RuntimeEffect, String> {
    let hit = EFFECTS.with(|m| m.borrow().get(program.source()).cloned());
    if let Some(e) = hit {
        return Ok(e);
    }
    let e = RuntimeEffect::make_for_shader(program.source().as_ref(), None)
        .map_err(|e| format!("brush program checked by eval failed to compile in render: {e}"))?;
    EFFECTS.with(|m| m.borrow_mut().insert(program.source().clone(), e.clone()));
    Ok(e)
}

/// The prelude's seed uniforms. `seed` has 256 values and `offset` 61 x 59, so hash inputs stay
/// small enough for the exact integer hash. Two brushes in one scene can share a pattern.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seeds {
    pub seed: f32,
    pub offset: [f32; 2],
}

impl From<u32> for Seeds {
    fn from(seed: u32) -> Seeds {
        Seeds {
            seed: (seed % 256) as f32,
            offset: [(seed % 61) as f32, ((seed / 61) % 59) as f32],
        }
    }
}

/// The first use of each identifier in `text` that starts with [`RESERVED_PREFIX`], in order,
/// with its body line and column. Comments are skipped.
pub fn reserved_identifiers(text: &str) -> Vec<(String, Origin)> {
    let mut out: Vec<(String, Origin)> = Vec::new();
    for (i, line) in blank_comments(text).lines().enumerate() {
        let mut rest = line;
        while let Some(start) = rest.find(|c: char| c.is_ascii_alphabetic() || c == '_') {
            let after = &rest[start..];
            let end = after
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(after.len());
            let word = &after[..end];
            let glued = rest[..start].ends_with(|c: char| c.is_ascii_digit());
            if !glued && word.starts_with(RESERVED_PREFIX) && !out.iter().any(|(w, _)| w == word) {
                let col = line[..line.len() - after.len()].chars().count() as u32;
                out.push((word.to_string(), Origin::Body { line: i as u32 + 1, col: Some(col) }));
            }
            rest = &after[end..];
        }
    }
    out
}

/// `text` with every `//` and `/* */` comment turned into spaces, lines and columns kept.
fn blank_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let block = match (c, chars.peek()) {
            ('/', Some('/')) => false,
            ('/', Some('*')) => true,
            _ => {
                out.push(c);
                continue;
            }
        };
        out.push(' ');
        if block {
            chars.next();
            out.push(' ');
        }
        let mut prev = ' ';
        while let Some(&n) = chars.peek() {
            if !block && n == '\n' {
                break;
            }
            chars.next();
            out.push(if n == '\n' { '\n' } else { ' ' });
            if block && prev == '*' && n == '/' {
                break;
            }
            prev = n;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const OK_BODY: &str = "float4 shade(Mark m) {\n  return float4(m.paint.rgb, cover(m.edge, m) * strength);\n}\n";

    fn params() -> Vec<(String, UniformKind)> {
        vec![("strength".into(), UniformKind::Float), ("tint".into(), UniformKind::Float4)]
    }

    #[test]
    fn a_sound_body_compiles_under_both_mains() {
        for kind in [MainKind::Area, MainKind::Ribbon] {
            let p = Program::assemble(kind, &params(), OK_BODY);
            assert_eq!(check(&p), Ok(()), "{kind:?}");
            assert!(effect(&p).is_ok());
        }
    }

    #[test]
    fn an_error_maps_to_its_body_line_and_column() {
        let body = "float4 shade(Mark m) {\n  float n = fbmx(m.uv, 3);\n  return float4(m.paint.rgb, n);\n}\n";
        let p = Program::assemble(MainKind::Area, &params(), body);
        let errs = check(&p).unwrap_err();
        assert_eq!(errs.len(), 2, "{errs:?}");
        assert_eq!(errs[0].origin, Origin::Body { line: 2, col: Some(12) });
        assert_eq!(errs[0].message, "unknown identifier 'fbmx'");
        assert_eq!(errs[1].origin, Origin::Body { line: 3, col: Some(29) });
    }

    #[test]
    fn a_tab_in_the_body_does_not_shift_the_column() {
        let body = "float4 shade(Mark m) {\n\treturn float4(m.paint.rgb, nn);\n}\nfloat g() {\n  \t\tx return 1; }\n";
        let errs = check(&Program::assemble(MainKind::Area, &[], body)).unwrap_err();
        assert_eq!(errs[0].origin, Origin::Body { line: 2, col: Some(28) }, "{errs:?}");
        assert_eq!(errs[1].origin, Origin::Body { line: 5, col: Some(4) }, "{errs:?}");
    }

    #[test]
    fn a_missing_shade_is_a_frame_error() {
        let p = Program::assemble(MainKind::Area, &[], "float f(float x) { return x; }\n");
        let errs = check(&p).unwrap_err();
        assert_eq!(errs.len(), 1, "only the first frame error: {errs:?}");
        assert_eq!(errs[0].origin, Origin::Frame);
        assert!(errs[0].message.contains("shade"), "{errs:?}");
    }

    #[test]
    fn a_param_that_breaks_its_uniform_line_maps_to_the_param() {
        let p = Program::assemble(
            MainKind::Area,
            &[("float".into(), UniformKind::Float), ("ok".into(), UniformKind::Float)],
            OK_BODY,
        );
        let errs = check(&p).unwrap_err();
        assert!(errs.iter().any(|e| e.origin == Origin::Param(0)), "{errs:?}");
    }

    #[test]
    fn the_prelude_declares_every_listed_field_and_helper() {
        let decl = PRELUDE.split("struct Mark {").nth(1).unwrap().split("};").next().unwrap();
        for f in MARK_FIELDS {
            assert!(decl.contains(&format!(" {f};")), "Mark has no field {f}");
        }
        assert_eq!(decl.matches(';').count(), MARK_FIELDS.len(), "MARK_FIELDS is out of date");
        for h in HELPERS {
            assert!(PRELUDE.contains(&format!(" {h}(")), "prelude has no helper {h}");
        }
    }

    #[test]
    fn seeds_reduce_as_the_contract_says() {
        assert_eq!(Seeds::from(0), Seeds { seed: 0.0, offset: [0.0, 0.0] });
        assert_eq!(Seeds::from(257), Seeds { seed: 1.0, offset: [13.0, 4.0] });
    }

    #[test]
    fn reserved_identifiers_are_found_in_a_body_outside_comments() {
        let at = |line, col| Origin::Body { line, col: Some(col) };
        assert_eq!(
            reserved_identifiers("float u_x = 1; u_x + u_y; au_z 2u_w"),
            vec![("u_x".to_string(), at(1, 6)), ("u_y".to_string(), at(1, 21))]
        );
        assert_eq!(
            reserved_identifiers("// u_prev is the base\n/*/ u_a\n u_b */ float k;\n  float q = u_seed;"),
            vec![("u_seed".to_string(), at(4, 12))]
        );
        assert!(reserved_identifiers(OK_BODY).is_empty());
    }
}
