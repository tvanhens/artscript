//! Field trees compiled to one SkSL shader. Noise is in the node's local canvas units.
//! The scalar result is mapped through stops by sampling a 256-wide ramp, or emitted as coverage.

use artscript_ir::{Field, FieldFn, FieldOp, NoiseKind, Stop};
use skia_safe::{
    Color4f, Data, Paint, Rect, RuntimeEffect, SamplingOptions, Shader, TileMode, surfaces,
};

use crate::paint::{color4, linear_gradient};
use crate::RenderError;

const PREAMBLE: &str = r#"
float hash21(float2 p) {
    float3 p3 = fract(float3(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

float2 grad2(float2 cell, float2 sd) {
    float a = hash21(cell + sd) * 6.28318530718;
    return float2(cos(a), sin(a));
}

float value2(float2 p, float2 sd) {
    float2 i = floor(p);
    float2 f = fract(p);
    float2 u = f * f * (3.0 - 2.0 * f);
    float a = hash21(i + sd);
    float b = hash21(i + float2(1.0, 0.0) + sd);
    float c = hash21(i + float2(0.0, 1.0) + sd);
    float d = hash21(i + float2(1.0, 1.0) + sd);
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

float perlin2(float2 p, float2 sd) {
    float2 i = floor(p);
    float2 f = fract(p);
    float2 u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    float n00 = dot(grad2(i, sd), f);
    float n10 = dot(grad2(i + float2(1.0, 0.0), sd), f - float2(1.0, 0.0));
    float n01 = dot(grad2(i + float2(0.0, 1.0), sd), f - float2(0.0, 1.0));
    float n11 = dot(grad2(i + float2(1.0, 1.0), sd), f - float2(1.0, 1.0));
    float n = mix(mix(n00, n10, u.x), mix(n01, n11, u.x), u.y);
    return clamp(0.5 + 0.5 * n * 1.41421356, 0.0, 1.0);
}

float simplex2(float2 p, float2 sd) {
    const float K1 = 0.36602540378;
    const float K2 = 0.21132486540;
    float2 i = floor(p + dot(p, float2(K1)));
    float2 a = p - i + dot(i, float2(K2));
    float2 o = (a.x > a.y) ? float2(1.0, 0.0) : float2(0.0, 1.0);
    float2 b = a - o + float2(K2);
    float2 c = a - 1.0 + 2.0 * float2(K2);
    float3 h = max(0.5 - float3(dot(a, a), dot(b, b), dot(c, c)), 0.0);
    float3 n = h * h * h * h * float3(
        dot(grad2(i, sd), a),
        dot(grad2(i + o, sd), b),
        dot(grad2(i + 1.0, sd), c)
    );
    return clamp(0.5 + 35.0 * 1.41421356 * (n.x + n.y + n.z), 0.0, 1.0);
}

float worley2(float2 p, float2 sd) {
    float2 i = floor(p);
    float m = 1.5;
    for (int y = -1; y <= 1; y++) {
        for (int x = -1; x <= 1; x++) {
            float2 g = i + float2(float(x), float(y));
            float2 fp = float2(hash21(g + sd), hash21(g + sd + float2(19.19, 7.13)));
            m = min(m, length(g + fp - p));
        }
    }
    return clamp(1.0 - m, 0.0, 1.0);
}
"#;

pub enum FieldUse<'a> {
    /// Map 0..1 through these sRGB stops.
    Stops(&'a [Stop]),
    /// Coverage mask. `threshold` turns the field into a hard step (`field > t`).
    /// `as_luma` writes an opaque gray so a luminance mask reads the value from RGB.
    Coverage { threshold: Option<f32>, as_luma: bool },
}

pub fn field_shader(field: &Field, use_: FieldUse<'_>) -> Result<Shader, RenderError> {
    let mut built = Gen { src: PREAMBLE.to_string(), n: 0 };
    let root = built.emit(field);
    let (main, child) = match use_ {
        FieldUse::Coverage { threshold, as_luma } => {
            let step = match threshold {
                Some(t) => format!("float a = v > {t:.8} ? 1.0 : 0.0;"),
                None => "float a = v;".to_string(),
            };
            let ret = if as_luma { "half4(a, a, a, 1.0)" } else { "half4(a)" };
            (
                format!(
                    "half4 main(float2 p) {{\n  float v = clamp({root}(p), 0.0, 1.0);\n  {step}\n  return {ret};\n}}\n"
                ),
                None,
            )
        }
        FieldUse::Stops(stops) => (
            format!(
                "uniform shader ramp;\nhalf4 main(float2 p) {{\n  float v = clamp({root}(p), 0.0, 1.0);\n  return ramp.eval(float2(v * 255.0, 0.0));\n}}\n"
            ),
            Some(ramp_shader(stops)?),
        ),
    };
    built.src.push_str(&main);
    let effect = RuntimeEffect::make_for_shader(&built.src, None).map_err(RenderError::Skia)?;
    let children: Vec<skia_safe::runtime_effect::ChildPtr> = match child {
        Some(shader) => vec![shader.into()],
        None => Vec::new(),
    };
    effect
        .make_shader(Data::new_copy(&[]), &children, None)
        .ok_or_else(|| RenderError::Skia("field shader".into()))
}

struct Gen {
    src: String,
    n: u32,
}

impl Gen {
    fn fresh(&mut self) -> String {
        let name = format!("f{}", self.n);
        self.n += 1;
        name
    }

    fn emit(&mut self, field: &Field) -> String {
        match field {
            Field::Const(v) => {
                let name = self.fresh();
                self.src.push_str(&format!("float {name}(float2 p) {{ return {v:.8}; }}\n"));
                name
            }
            Field::Noise { kind, scale, seed } => {
                let name = self.fresh();
                let scale = if *scale == 0.0 { 0.0001 } else { *scale };
                let (lo, hi) = seed_pair(*seed);
                let func = match kind {
                    NoiseKind::Value => "value2",
                    NoiseKind::Perlin => "perlin2",
                    NoiseKind::Simplex => "simplex2",
                    NoiseKind::Worley => "worley2",
                };
                self.src.push_str(&format!(
                    "float {name}(float2 p) {{ return {func}(p / {scale:.8}, float2({lo:.1}, {hi:.1})); }}\n"
                ));
                name
            }
            Field::Fbm { base, octaves, ridged } => {
                let inner = self.emit(base);
                let name = self.fresh();
                let mut body = String::from("float sum = 0.0;\n  float norm = 0.0;\n");
                let n = *octaves;
                if n == 0 {
                    self.src.push_str(&format!("float {name}(float2 p) {{ return 0.0; }}\n"));
                    return name;
                }
                let mut amp = 1.0_f32;
                for i in 0..n {
                    let freq = 2.0_f32.powi(i as i32);
                    body.push_str(&format!("  float v{i} = {inner}(p * {freq:.1});\n"));
                    if *ridged {
                        body.push_str(&format!("  v{i} = 1.0 - abs(2.0 * v{i} - 1.0);\n"));
                    }
                    body.push_str(&format!("  sum += v{i} * {amp:.8};\n  norm += {amp:.8};\n"));
                    amp *= 0.5;
                }
                self.src.push_str(&format!(
                    "float {name}(float2 p) {{\n  {body}  return norm == 0.0 ? 0.0 : sum / norm;\n}}\n"
                ));
                name
            }
            Field::Unary(op, inner) => {
                let inner = self.emit(inner);
                let name = self.fresh();
                let expr = match op {
                    FieldFn::Abs => format!("abs({inner}(p))"),
                    FieldFn::Neg => format!("-{inner}(p)"),
                    FieldFn::Clamp => format!("clamp({inner}(p), 0.0, 1.0)"),
                };
                self.src.push_str(&format!("float {name}(float2 p) {{ return {expr}; }}\n"));
                name
            }
            Field::Binary(op, a, b) => {
                let a = self.emit(a);
                let b = self.emit(b);
                let name = self.fresh();
                let expr = match op {
                    FieldOp::Add => format!("{a}(p) + {b}(p)"),
                    FieldOp::Sub => format!("{a}(p) - {b}(p)"),
                    FieldOp::Mul => format!("{a}(p) * {b}(p)"),
                    FieldOp::Div => format!("({b}(p) == 0.0 ? 0.0 : {a}(p) / {b}(p))"),
                    FieldOp::Min => format!("min({a}(p), {b}(p))"),
                    FieldOp::Max => format!("max({a}(p), {b}(p))"),
                };
                self.src.push_str(&format!("float {name}(float2 p) {{ return {expr}; }}\n"));
                name
            }
        }
    }
}

fn seed_pair(seed: u32) -> (f32, f32) {
    ((seed & 0xffff) as f32, (seed >> 16) as f32)
}

fn ramp_shader(stops: &[Stop]) -> Result<Shader, RenderError> {
    // A 256-entry lookup table, kept on the CPU so every backend samples the same colors.
    let mut surface = surfaces::raster_n32_premul((256, 1))
        .ok_or_else(|| RenderError::Skia("ramp surface".into()))?;
    let canvas = surface.canvas();
    if stops.is_empty() {
        canvas.clear(Color4f { r: 0.0, g: 0.0, b: 0.0, a: 0.0 });
    } else if stops.len() == 1 {
        let c = color4(stops[0].color);
        canvas.clear(c);
    } else {
        let colors: Vec<Color4f> = stops.iter().map(|s| color4(s.color)).collect();
        let pos: Vec<f32> = stops.iter().map(|s| s.pos).collect();
        let shader = linear_gradient(
            &colors,
            &pos,
            skia_safe::Point::new(0.0, 0.0),
            skia_safe::Point::new(255.0, 0.0),
        )?;
        let mut paint = Paint::default();
        paint.set_shader(shader);
        canvas.draw_rect(Rect::from_wh(256.0, 1.0), &paint);
    }
    let image = surface.image_snapshot();
    let sampling = SamplingOptions::new(skia_safe::FilterMode::Linear, skia_safe::MipmapMode::None);
    image
        .to_shader((TileMode::Clamp, TileMode::Clamp), sampling, None)
        .ok_or_else(|| RenderError::Skia("ramp shader".into()))
}
