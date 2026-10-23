//! The SkSL backend: the prelude every body runs with, the two frames, the two mains, and the
//! printer that turns an [`ir::Body`] into `float4 shade(Mark m)`.
//!
//! A body runs under two frames. The shader frame runs the stage as a runtime shader over the
//! slot's field, with `m.below` transparent. The blender frame runs it as a runtime blender whose
//! `dst` is the backdrop, so `m.below` is what is already drawn under the pixel; render uses it
//! for every stage of a stack in which some stage reads `below`. The paint shader of such a draw
//! is [`ENCODE`], which writes the device pixel into the color for the frame to decode, since a
//! blender has no coordinate of its own.
//!
//! The hash, noise, `psin`, dither and 8-bit snap in the prelude are the CPU/GPU parity
//! contract: every intermediate of the lattice hash is an integer below 2^24, remainders are
//! divide-free, trig never calls the GPU's fast `sin`, and the output is snapped to 8-bit levels
//! after an in-shader dither so the two backends agree on whole levels. Do not edit those
//! functions without rerunning the parity comparison.

use std::collections::HashMap;
use std::fmt::Write;

use crate::ir::{self, Block, Cover, Local, LoopVar, NoiseSeed, Op, Stmt};
use crate::lang::{Builtin, Noise, Reduce, Ty};
use crate::{Frame, MainKind};

/// The exact lattice hash: `hcore` takes an integer point and returns 0..1, bit-identical on
/// CPU and GPU. Render dithers content paints with it, so both dithers share one hash.
macro_rules! hash_sksl {
    () => {
        r#"// Exact hashes: the input is quantized to 1/16 and every intermediate is an integer below
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
"#
    };
}

pub const HASH: &str = hash_sksl!();

/// Reads the device pixel back out of the color [`ENCODE`] wrote into `src`.
macro_rules! decode_sksl {
    () => {
        r#"// The device pixel a blender frame runs at, decoded from the color `ENCODE` wrote into `src`.
float2 u_decode(half4 s) {
    float3 v = floor(float3(s.rgb) * 2048 + 0.5);
    float hx = floor(v.z / 32);
    return float2(v.x + hx * 2048, v.y + (v.z - hx * 32) * 2048) + 0.5;
}
"#
    };
}

/// The paint shader of a blender-frame draw. It writes the device pixel into the color in
/// 11-bit steps, which are exact in half precision, with alpha 1 so it stays a valid
/// premultiplied color, and `u_decode` in the prelude reads it back.
pub const ENCODE: &str = "half4 main(float2 c) { float2 q = floor(c); float2 hi = floor(q / 2048); float2 lo = q - hi * 2048; return half4(lo.x / 2048, lo.y / 2048, (hi.x * 32 + hi.y) / 2048, 1); }\n";

/// The decoder, for a blender outside this crate that binds the same paint shader.
pub const DECODE: &str = decode_sksl!();

/// Uniforms the runtime binds, `struct Mark`, and the helpers the printed body calls. `u_prev`
/// is a shader under the shader frame and a blender under the blender frame.
macro_rules! prelude_sksl {
    ($prev:literal) => {
        concat!(
            "\nuniform ",
            $prev,
            r#" u_prev;
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
uniform shader u_norm;
uniform float2 u_fieldo;
uniform float2 u_rib;
uniform float u_s0;
uniform float2 u_dito;

struct Mark {
    float3 position; float3 normal; float2 uv; float2 size; float4 paint; float seed; float px;
    float edge; float2 edge_normal; float s; float d;
    float pressure; float angle; float tilt; float velocity;
    float4 below;
};

"#,
            hash_sksl!(),
            r#"float hash12(float2 p) { return hcore(floor(p * 16 + 0.5)); }
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
float2 psin(float2 t) { return float2(psin(t.x), psin(t.y)); }
float3 psin(float3 t) { return float3(psin(t.x), psin(t.y), psin(t.z)); }
float2 rot(float2 p, float deg) {
    float tr = deg * (1.0 / 360); float c = psin(tr + 0.25); float s = psin(tr);
    return float2(c * p.x - s * p.y, s * p.x + c * p.y);
}
// Signed distance (local units, + inside) to coverage, antialiased over one pixel.
float cover(float d, Mark m) { return saturate(d / m.px + 0.5); }
"#,
            decode_sksl!(),
            r#"// The stage's output: coverage times the paint's alpha, premultiplied, dithered on the last
// stage only, and snapped to 8-bit levels.
half4 u_pack(float4 o, Mark m, float2 c) {
    float k = saturate(o.a) * m.paint.a;
    float3 rgb = o.rgb;
    if (u_last > 0.5) {
        // Exact per-pixel dither: Skia's CPU and GPU dithers differ, this one is bit-identical.
        // It is anchored at `u_dito`, the device pixel of local (0,0), so a copy dithers alike.
        rgb += (hcore(floor(c) - u_dito) - 0.5) / 255;
    }
    // Snap to 8-bit levels so the GPU's half-float color path cannot round across a tie.
    float4 lv = floor(float4(saturate(rgb) * k, k) * 255 + 0.5);
    return half4(lv / 255);
}
"#
        )
    };
}

const PRELUDE: &str = prelude_sksl!("shader");
const PRELUDE_BLEND: &str = prelude_sksl!("blender");

/// The shader frame's tail, after the body so it can call `shade`. `u_base` set reads the base
/// paint (or the previous stage) from `u_prev`, snapped to 8-bit levels first so a half-float
/// child read gives the same levels as a float one. A shader has no backdrop, so `m.below` is
/// transparent.
const FINISH: &str = r#"
half4 u_finish(Mark m, float2 c) {
    m.seed = u_seed; m.px = u_px;
    m.paint = u_paint; m.below = float4(0);
    if (u_base > 0.5) {
        float4 pv = floor(float4(u_prev.eval(c)) * 255 + 0.5);
        m.paint = float4(pv.a > 0 ? pv.rgb / pv.a : float3(0), pv.a / 255);
    }
    return u_pack(shade(m), m, c);
}
"#;

/// The blender frame's tail. `dst` is the backdrop and is the same for every stage of a stack:
/// the previous stage is a child blender evaluated over it, and `m.below` is it as straight
/// rgba at 8-bit levels. The output is the stage's own premultiplied color, not composited.
const FINISH_BLEND: &str = r#"
half4 u_finish(Mark m, float2 c, half4 src, half4 dst) {
    m.seed = u_seed; m.px = u_px;
    m.paint = u_paint;
    if (u_base > 0.5) {
        float4 pv = floor(float4(u_prev.eval(src, dst)) * 255 + 0.5);
        m.paint = float4(pv.a > 0 ? pv.rgb / pv.a : float3(0), pv.a / 255);
    }
    float4 bv = floor(float4(dst) * 255 + 0.5);
    m.below = float4(bv.a > 0 ? bv.rgb / bv.a : float3(0), bv.a / 255);
    return u_pack(shade(m), m, c);
}
"#;

/// The Mark of an area body. The signed distance comes from a device-aligned field, one texel
/// per device pixel, so `u_sdf.eval(c - u_sdfo)` is an exact texel read on both backends. The
/// 2D frame is x right, y down, z toward the viewer, so the normal is +z.
const AREA_MARK: &str = r#"
Mark u_mark(float2 c) {
    float2 p = (u_dtl * float3(c, 1)).xy;
    float4 e = u_sdf.eval(c - u_sdfo);
    Mark m;
    m.position = float3(p, 0); m.normal = float3(0, 0, 1);
    float2 q0 = p - u_frame.xy;
    float2 r = float2(u_dir.y * q0.x + u_dir.z * q0.y, -u_dir.z * q0.x + u_dir.y * q0.y);
    m.uv = r; m.s = r.x + u_frame.z * .5; m.d = r.y;
    m.size = float2(u_frame.z, u_frame.w);
    m.pressure = 1; m.angle = u_dir.x; m.tilt = 0; m.velocity = 1;
    m.edge = e.r * u_px; m.edge_normal = e.gb;
    return m;
}
"#;

const AREA_MAIN: &str = "half4 main(float2 c) { return u_finish(u_mark(c), c); }\n";

const AREA_MAIN_BLEND: &str =
    "half4 main(half4 src, half4 dst) { float2 c = u_decode(src); return u_finish(u_mark(c), c, src, dst); }\n";

/// The Mark of a ribbon body, or false past the ribbon's reach. `u_field` is a device-aligned
/// field, one texel per device pixel: arc length `s` to the spine in two half-float channels
/// (whole chunks of [`S_CHUNK`](crate::S_CHUNK) device pixels past `u_s0` chunks, then the
/// rest), the signed offset `d` across it, and the half width there, all in device pixels. A
/// negative rest marks a pixel past the ribbon's reach. `u_chan` holds pressure, angle, tilt and
/// velocity at the same texel, and `u_norm` the unit direction in local units from the spine to
/// the texel, which is the normal of the edge on a stroke whatever its taper. `u_rib` is the
/// contour's length in local units and 1 when it is closed. Past the end of an open contour `s`
/// keeps running, so `edge` rounds the cap.
const RIBBON_MARK: &str = r#"
bool u_mark(float2 c, out Mark m) {
    float4 g = u_field.eval(c - u_fieldo);
    if (g.g < 0) { return false; }
    float4 ch = u_chan.eval(c - u_fieldo);
    m.position = float3((u_dtl * float3(c, 1)).xy, 0); m.normal = float3(0, 0, 1);
    m.s = ((g.r + u_s0) * 32 + g.g) * u_px; m.d = g.b * u_px;
    float hw = max(g.a * u_px, 1e-6);
    m.size = float2(u_rib.x, 2 * hw);
    m.pressure = ch.x; m.angle = ch.y; m.tilt = ch.z; m.velocity = ch.w;
    float over = u_rib.y > 0.5 ? 0 : (m.s < 0 ? m.s : max(m.s - u_rib.x, 0));
    m.edge = hw - length(float2(over, m.d));
    m.edge_normal = u_norm.eval(c - u_fieldo).xy;
    m.uv = float2(m.s, m.d);
    return true;
}
"#;

const RIBBON_MAIN: &str = "half4 main(float2 c) { Mark m; if (!u_mark(c, m)) { return half4(0); } return u_finish(m, c); }\n";

const RIBBON_MAIN_BLEND: &str = "half4 main(half4 src, half4 dst) { float2 c = u_decode(src); Mark m; if (!u_mark(c, m)) { return half4(0); } return u_finish(m, c, src, dst); }\n";

/// The prelude for `frame`, then `shade`, the frame's finish, `main`'s Mark and its main.
/// Deterministic: the same inputs give the same text.
pub(crate) fn assemble(shade: &str, main: MainKind, frame: Frame) -> String {
    let (mark, main_text) = match (main, frame) {
        (MainKind::Area, Frame::Shader) => (AREA_MARK, AREA_MAIN),
        (MainKind::Area, Frame::Blender) => (AREA_MARK, AREA_MAIN_BLEND),
        (MainKind::Ribbon, Frame::Shader) => (RIBBON_MARK, RIBBON_MAIN),
        (MainKind::Ribbon, Frame::Blender) => (RIBBON_MARK, RIBBON_MAIN_BLEND),
    };
    let (prelude, finish) = match frame {
        Frame::Shader => (PRELUDE, FINISH),
        Frame::Blender => (PRELUDE_BLEND, FINISH_BLEND),
    };
    let mut src = String::with_capacity(prelude.len() + shade.len() + finish.len() + mark.len() + main_text.len());
    src.push_str(prelude);
    src.push_str(shade);
    src.push_str(finish);
    src.push_str(mark);
    src.push_str(main_text);
    src
}

/// `uniform float p_strength; ...` then `float4 shade(Mark m) { ... }`. Every author name is
/// prefixed or numbered, so no body name can reach the prelude or the runtime's `u_` uniforms.
pub(crate) fn print(body: &ir::Body) -> String {
    let mut p = Printer { body, out: String::new(), indent: 1, types: HashMap::new() };
    for (name, ty) in &body.params {
        writeln!(p.out, "uniform {} p_{name};", sksl_ty(*ty)).unwrap();
    }
    for i in 0..body.salts.len() {
        writeln!(p.out, "uniform float2 u_salt{i};").unwrap();
    }
    p.out.push_str("float4 shade(Mark m) {\n");
    p.stmts(&body.stmts);
    let cover = match body.out.cover {
        Cover::Value(l) => local(l),
        Cover::Surface => "cover(m.edge, m)".to_string(),
    };
    p.line(&format!("return float4({}, {cover});", local(body.out.rgb)));
    p.out.push_str("}\n");
    p.out
}

fn sksl_ty(ty: Ty) -> &'static str {
    match ty {
        Ty::Num => "float",
        Ty::Vec2 => "float2",
        Ty::Vec3 => "float3",
        Ty::Color => "float4",
        Ty::Bool => "bool",
    }
}

fn local(l: Local) -> String {
    format!("v{}", l.0)
}

fn loop_var(v: LoopVar) -> String {
    format!("i{}", v.0)
}

/// A literal as the author wrote it, so the target parses the same decimal.
fn num(x: f64) -> String {
    format!("{x}")
}

struct Printer<'a> {
    body: &'a ir::Body,
    out: String,
    indent: usize,
    /// The type of each local printed so far. Lookups only.
    types: HashMap<Local, Ty>,
}

impl Printer<'_> {
    fn line(&mut self, text: &str) {
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
        self.out.push_str(text);
        self.out.push('\n');
    }

    fn stmts(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            self.stmt(s);
        }
    }

    fn block(&mut self, b: &Block, dst: Local, finish: impl FnOnce(&str) -> String) {
        self.indent += 1;
        self.stmts(&b.stmts);
        let text = finish(&local(b.value));
        self.line(&format!("{} = {text};", local(dst)));
        self.indent -= 1;
    }

    fn stmt(&mut self, s: &Stmt) {
        self.types.insert(s.dst, s.ty);
        let ty = sksl_ty(s.ty);
        let dst = local(s.dst);
        match &s.op {
            Op::If { cond, then, otherwise } => {
                self.line(&format!("{ty} {dst};"));
                self.line(&format!("if ({}) {{", local(*cond)));
                self.block(then, s.dst, str::to_string);
                self.line("} else {");
                self.block(otherwise, s.dst, str::to_string);
                self.line("}");
            }
            Op::Reduce { op, var, lo, hi, item } => {
                let i = loop_var(*var);
                self.line(&format!("{ty} {dst} = 0;"));
                self.line(&format!("for (int {i} = {lo}; {i} < {hi}; {i}++) {{"));
                self.block(item, s.dst, |v| match op {
                    Reduce::Sum => format!("{dst} + {v}"),
                    Reduce::Min | Reduce::Max => format!("{i} == {lo} ? {v} : {}({dst}, {v})", op.name()),
                });
                self.line("}");
            }
            other => {
                let expr = self.expr(other);
                self.line(&format!("{ty} {dst} = {expr};"));
            }
        }
    }

    fn expr(&self, op: &Op) -> String {
        match op {
            Op::Num(x) => num(*x),
            Op::Bool(b) => b.to_string(),
            Op::Param(i) => format!("p_{}", self.body.params[*i as usize].0),
            Op::Field(f) => format!("m.{}", f.name()),
            Op::Loop(v) => format!("float({})", loop_var(*v)),
            Op::Neg(a) => format!("-{}", local(*a)),
            Op::Not(a) => format!("!{}", local(*a)),
            Op::Arith(op, a, b) => format!("{} {} {}", local(*a), op.symbol(), local(*b)),
            Op::Logic(op, a, b) => {
                let sym = match op {
                    ir::Logic::Lt => "<",
                    ir::Logic::Le => "<=",
                    ir::Logic::Gt => ">",
                    ir::Logic::Ge => ">=",
                    ir::Logic::Eq => "==",
                    ir::Logic::Ne => "!=",
                    ir::Logic::And => "&&",
                    ir::Logic::Or => "||",
                };
                format!("{} {sym} {}", local(*a), local(*b))
            }
            Op::Swizzle(a, sw) => {
                let letters: String = sw.lanes[..sw.len as usize].iter().map(|l| ['x', 'y', 'z', 'w'][*l as usize]).collect();
                format!("{}.{letters}", local(*a))
            }
            Op::Pack(items) => {
                let lanes: u8 = items.iter().map(|l| self.types[l].lanes()).sum();
                format!("float{lanes}({})", items.iter().map(|l| local(*l)).collect::<Vec<_>>().join(", "))
            }
            Op::Call(f, args) => self.call(*f, args),
            Op::Noise { f, at, seed, oct } => {
                let p = local(*at);
                let point = match seed {
                    NoiseSeed::Stage => format!("{p} + u_seedo"),
                    NoiseSeed::Salt(k) => {
                        let i = self.body.salts.binary_search(k).expect("a used salt is listed");
                        format!("{p} + u_salt{i}")
                    }
                    NoiseSeed::Fixed([0, 0]) => p,
                    NoiseSeed::Fixed([x, y]) => format!("{p} + float2({x}, {y})"),
                };
                match f {
                    Noise::Hash => format!("hash12({point})"),
                    Noise::Hash2 => format!("hash22({point})"),
                    Noise::Value => format!("vnoise({point})"),
                    Noise::Simplex => format!("simplex({point})"),
                    Noise::Worley => format!("worley({point})"),
                    Noise::Fbm => format!("fbm({point}, {oct})"),
                }
            }
            Op::Fold { op, items } => {
                let mut acc = local(items[0]);
                for item in &items[1..] {
                    acc = match op {
                        Reduce::Sum => format!("{acc} + {}", local(*item)),
                        Reduce::Min | Reduce::Max => format!("{}({acc}, {})", op.name(), local(*item)),
                    };
                }
                acc
            }
            Op::If { .. } | Op::Reduce { .. } => unreachable!("blocks print as statements"),
        }
    }

    /// Each builtin keeps its `values` meaning: degrees for trig, `sqrt` clamped at 0, `round`
    /// half away from zero, `lerp` as the target's `mix`.
    fn call(&self, f: Builtin, args: &[Local]) -> String {
        let lanes = args.iter().map(|l| self.types[l].lanes()).max().unwrap_or(1);
        // GLSL has one overload with scalars at these positions, all of them at once; a number
        // anywhere else, or beside a vector at one of them, is splatted.
        let scalar_ok: &[usize] = match f {
            Builtin::Min | Builtin::Max | Builtin::Mod | Builtin::Rot => &[1],
            Builtin::Clamp => &[1, 2],
            Builtin::Smoothstep => &[0, 1],
            Builtin::Lerp => &[2],
            Builtin::Step => &[0],
            _ => &[],
        };
        let keep = scalar_ok.iter().all(|i| args.get(*i).is_none_or(|l| self.types[l].lanes() == 1));
        let a: Vec<String> = args
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let v = local(*l);
                let splat = lanes > 1 && self.types[l].lanes() == 1 && !(keep && scalar_ok.contains(&i));
                if splat { format!("float{lanes}({v})") } else { v }
            })
            .collect();
        match f {
            Builtin::Lerp => format!("mix({}, {}, {})", a[0], a[1], a[2]),
            Builtin::Sqrt => format!("sqrt(max({}, 0.0))", a[0]),
            Builtin::Round => format!("(floor(abs({0}) + 0.5) * sign({0}))", a[0]),
            Builtin::Sin => format!("psin({} * (1.0 / 360))", a[0]),
            Builtin::Cos => format!("psin({} * (1.0 / 360) + 0.25)", a[0]),
            Builtin::Atan2 => format!("(atan({}, {}) * 57.29577951308232)", a[0], a[1]),
            Builtin::Rad => format!("({} * 0.017453292519943295)", a[0]),
            Builtin::Cover => format!("cover({}, m)", a[0]),
            other => format!("{}({})", other.name(), a.join(", ")),
        }
    }

}

impl ir::Arith {
    pub(crate) fn symbol(self) -> &'static str {
        match self {
            ir::Arith::Add => "+",
            ir::Arith::Sub => "-",
            ir::Arith::Mul => "*",
            ir::Arith::Div => "/",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prelude_declares_every_field_and_the_frames_share_a_mark() {
        let decl = PRELUDE.split("struct Mark {").nth(1).unwrap().split("};").next().unwrap();
        for f in crate::lang::Field::ALL {
            if *f == crate::lang::Field::Light {
                continue;
            }
            assert!(decl.contains(&format!(" {};", f.name())), "Mark has no field {}", f.name());
        }
        assert_eq!(PRELUDE.lines().count(), PRELUDE_BLEND.lines().count());
    }

    #[test]
    fn literals_print_as_the_author_wrote_them() {
        assert_eq!(num(0.46), "0.46");
        assert_eq!(num(1.0), "1");
        assert_eq!(num(-1.3), "-1.3");
        assert_eq!(num(0.0000001), "0.0000001");
    }
}
