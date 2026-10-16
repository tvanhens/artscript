//! OKLab / OKLCH, hue-shifted ramps, lineart colors and OKLCH gradient densification.
//! Ported from chibi `color.py`; the ramp step formula is the one in docs/semantics.md section 6.

const COOL_HUE: f64 = 275.0;
const WARM_HUE: f64 = 75.0;
pub const DEFAULT_HUE_STEP: f64 = 15.0;
pub const GRADIENT_SUBSTOPS: usize = 16;

/// sRGB 0..1 with straight alpha.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl Rgba {
    pub const GRAY: Rgba = Rgba { r: 138.0 / 255.0, g: 138.0 / 255.0, b: 138.0 / 255.0, a: 1.0 };

    pub fn hex(code: &str) -> Option<Rgba> {
        let h = code.trim_start_matches('#');
        let expanded: String = match h.len() {
            3 | 4 => h.chars().flat_map(|c| [c, c]).collect(),
            6 | 8 => h.to_string(),
            _ => return None,
        };
        let v = |i: usize| u8::from_str_radix(&expanded[i..i + 2], 16).ok().map(|x| x as f64 / 255.0);
        Some(Rgba {
            r: v(0)?,
            g: v(2)?,
            b: v(4)?,
            a: if expanded.len() == 8 { v(6)? } else { 1.0 },
        })
    }

    pub fn with_alpha(self, a: f64) -> Rgba {
        Rgba { a, ..self }
    }

    pub fn to_ir(self) -> artscript_ir::Color {
        artscript_ir::Color { r: self.r as f32, g: self.g as f32, b: self.b as f32, a: self.a as f32 }
    }

    pub fn from_ir(c: artscript_ir::Color) -> Rgba {
        Rgba { r: c.r as f64, g: c.g as f64, b: c.b as f64, a: c.a as f64 }
    }

    pub fn to_hex(self) -> String {
        self.to_ir().hex()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lch {
    pub l: f64,
    pub c: f64,
    pub h: f64,
}

fn to_linear(u: f64) -> f64 {
    if u <= 0.04045 { u / 12.92 } else { ((u + 0.055) / 1.055).powf(2.4) }
}

fn to_srgb(u: f64) -> f64 {
    if u <= 0.0031308 { 12.92 * u } else { 1.055 * u.max(0.0).powf(1.0 / 2.4) - 0.055 }
}

pub fn oklab(c: Rgba) -> [f64; 3] {
    let (r, g, b) = (to_linear(c.r), to_linear(c.g), to_linear(c.b));
    let l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b;
    let m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b;
    let s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b;
    let (l, m, s) = (l.cbrt(), m.cbrt(), s.cbrt());
    [
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    ]
}

fn from_oklab_raw(l: f64, a: f64, b: f64) -> [f64; 3] {
    let lp = (l + 0.3963377774 * a + 0.2158037573 * b).powi(3);
    let mp = (l - 0.1055613458 * a - 0.0638541728 * b).powi(3);
    let sp = (l - 0.0894841775 * a - 1.2914855480 * b).powi(3);
    [
        4.0767416621 * lp - 3.3077115913 * mp + 0.2309699292 * sp,
        -1.2684380046 * lp + 2.6097574011 * mp - 0.3413193965 * sp,
        -0.0041960863 * lp - 0.7034186147 * mp + 1.7076147010 * sp,
    ]
}

pub fn oklch(c: Rgba) -> Lch {
    let [l, a, b] = oklab(c);
    Lch { l, c: a.hypot(b), h: b.atan2(a).to_degrees().rem_euclid(360.0) }
}

/// Maps out-of-gamut colors by reducing chroma (binary search), never by clipping.
pub fn from_oklch(lch: Lch, alpha: f64) -> Rgba {
    let l = lch.l.clamp(0.0, 1.0);
    let hr = lch.h.to_radians();
    let rgb_at = |chroma: f64| from_oklab_raw(l, chroma * hr.cos(), chroma * hr.sin());
    let fits = |chroma: f64| rgb_at(chroma).iter().all(|v| (-1e-4..=1.0 + 1e-4).contains(v));
    let mut chroma = lch.c.max(0.0);
    if !fits(chroma) {
        let (mut lo, mut hi) = (0.0, chroma);
        for _ in 0..20 {
            let mid = (lo + hi) / 2.0;
            if fits(mid) { lo = mid } else { hi = mid }
        }
        chroma = lo;
    }
    let [r, g, b] = rgb_at(chroma).map(|v| to_srgb(v.clamp(0.0, 1.0)).clamp(0.0, 1.0));
    Rgba { r, g, b, a: alpha }
}

fn toward(h: f64, target: f64, degrees: f64) -> f64 {
    let diff = (target - h + 180.0).rem_euclid(360.0) - 180.0;
    (h + diff.signum() * diff.abs().min(degrees)).rem_euclid(360.0)
}

/// One ramp step `s` away from `base` (fractional and out-of-range steps use the same formula).
pub fn step(base: Rgba, s: f64, hue: f64) -> Rgba {
    if s == 0.0 {
        return base;
    }
    let Lch { l, c, h } = oklch(base);
    let neutral = c < 0.02;
    let k = s.abs();
    let lch = if s < 0.0 {
        Lch {
            l: l - 0.125 * k,
            c: c * (1.0 - 0.1 * k) + 0.005 * k,
            h: if neutral { h } else { toward(h, COOL_HUE, hue * k) },
        }
    } else {
        Lch {
            l: l + (0.075 * k).min((0.985 - l) * 0.4 * k),
            c: c * (1.0 - 0.12 * k),
            h: if neutral { h } else { toward(h, WARM_HUE, 0.65 * hue * k) },
        }
    };
    from_oklch(lch, base.a)
}

/// Lineart color: darker, more saturated, rotated toward blue-violet.
pub fn line(c: Rgba) -> Rgba {
    let Lch { l, c: ch, h } = oklch(c);
    let h = if ch < 0.02 { h } else { toward(h, COOL_HUE, 24.0) };
    from_oklch(Lch { l: (l - 0.42).max(0.18), c: (ch * 1.15).max(0.04), h }, c.a)
}

/// Mixes in OKLab so mixes stay clean instead of going gray.
pub fn mix(a: Rgba, b: Rgba, t: f64) -> Rgba {
    let (la, lb) = (oklab(a), oklab(b));
    let [l, aa, bb] = [0, 1, 2].map(|i| la[i] + (lb[i] - la[i]) * t);
    from_oklch(
        Lch { l, c: aa.hypot(bb), h: bb.atan2(aa).to_degrees().rem_euclid(360.0) },
        a.a + (b.a - a.a) * t,
    )
}

/// OKLCH interpolation on the shorter hue arc with linear alpha. A transparent end takes its
/// color from the other end; a near-neutral end takes its hue from the other end.
pub fn lerp_oklch(a: Rgba, b: Rgba, t: f64) -> Rgba {
    let (mut pa, mut pb) = (oklch(a), oklch(b));
    if a.a == 0.0 && b.a > 0.0 {
        pa = pb;
    } else if b.a == 0.0 && a.a > 0.0 {
        pb = pa;
    } else if pa.c < 0.02 && pb.c >= 0.02 {
        pa.h = pb.h;
    } else if pb.c < 0.02 && pa.c >= 0.02 {
        pb.h = pa.h;
    }
    let dh = (pb.h - pa.h + 180.0).rem_euclid(360.0) - 180.0;
    from_oklch(
        Lch { l: pa.l + (pb.l - pa.l) * t, c: pa.c + (pb.c - pa.c) * t, h: pa.h + dh * t },
        a.a + (b.a - a.a) * t,
    )
}

#[derive(Clone, Debug, PartialEq)]
pub enum RampKind {
    /// `ramp c n [hue:h]`: every stop is `step(c, i - base, hue)`.
    Single { color: Rgba, hue: f64 },
    /// `ramp a b n`: OKLCH interpolation between the endpoints.
    Two,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ramp {
    pub stops: Vec<Rgba>,
    pub base: usize,
    pub kind: RampKind,
}

impl Ramp {
    pub fn single(color: Rgba, n: usize, hue: f64) -> Ramp {
        let n = n.max(1);
        let base = (n - 1) / 2;
        let stops = (0..n).map(|i| step(color, i as f64 - base as f64, hue)).collect();
        Ramp { stops, base, kind: RampKind::Single { color, hue } }
    }

    pub fn two(a: Rgba, b: Rgba, n: usize) -> Ramp {
        let n = n.max(2);
        let stops = (0..n).map(|i| lerp_oklch(a, b, i as f64 / (n - 1) as f64)).collect();
        Ramp { stops, base: (n - 1) / 2, kind: RampKind::Two }
    }

    pub fn base_color(&self) -> Rgba {
        self.stops[self.base]
    }

    pub fn line(&self) -> Rgba {
        line(self.base_color())
    }

    /// Stop at a possibly fractional or out-of-range index.
    pub fn at(&self, index: f64) -> Rgba {
        match &self.kind {
            RampKind::Single { color, hue } => step(*color, index - self.base as f64, *hue),
            RampKind::Two => {
                let last = (self.stops.len() - 1) as f64;
                if index < 0.0 {
                    step(self.stops[0], index, DEFAULT_HUE_STEP)
                } else if index > last {
                    step(self.stops[self.stops.len() - 1], index - last, DEFAULT_HUE_STEP)
                } else {
                    let i = index.floor() as usize;
                    let f = index - i as f64;
                    if f == 0.0 || i + 1 >= self.stops.len() {
                        self.stops[i]
                    } else {
                        lerp_oklch(self.stops[i], self.stops[i + 1], f)
                    }
                }
            }
        }
    }
}

/// Fills missing stop positions (first 0, last 1, the rest evenly between known neighbours),
/// then densifies each segment to 16 OKLCH sub-stops.
pub fn densify(stops: &[(Rgba, Option<f64>)]) -> Vec<(Rgba, f64)> {
    if stops.is_empty() {
        return Vec::new();
    }
    if stops.len() == 1 {
        return vec![(stops[0].0, 0.0), (stops[0].0, 1.0)];
    }
    let n = stops.len();
    let mut pos: Vec<Option<f64>> = stops.iter().map(|s| s.1).collect();
    pos[0] = Some(pos[0].unwrap_or(0.0));
    pos[n - 1] = Some(pos[n - 1].unwrap_or(1.0));
    let mut i = 0;
    while i < n {
        if pos[i].is_none() {
            let start = i - 1;
            let end = (i..n).find(|&j| pos[j].is_some()).unwrap_or(n - 1);
            let (p0, p1) = (pos[start].unwrap(), pos[end].unwrap());
            for (k, slot) in pos.iter_mut().enumerate().take(end).skip(i) {
                *slot = Some(p0 + (p1 - p0) * (k - start) as f64 / (end - start) as f64);
            }
            i = end;
        }
        i += 1;
    }
    let mut prev = 0.0f64;
    let pos: Vec<f64> = pos
        .into_iter()
        .map(|p| {
            prev = p.unwrap().clamp(prev, 1.0);
            prev
        })
        .collect();
    let mut out = Vec::with_capacity((n - 1) * GRADIENT_SUBSTOPS + 1);
    for k in 0..n - 1 {
        let (a, b) = (stops[k].0, stops[k + 1].0);
        for j in 0..GRADIENT_SUBSTOPS {
            let t = j as f64 / GRADIENT_SUBSTOPS as f64;
            out.push((lerp_oklch(a, b, t), pos[k] + (pos[k + 1] - pos[k]) * t));
        }
    }
    out.push((stops[n - 1].0, pos[n - 1]));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(c: Rgba) -> [i32; 4] {
        [c.r, c.g, c.b, c.a].map(|v| (v * 255.0).round() as i32)
    }

    fn close(c: Rgba, expected: [i32; 3]) {
        let got = bytes(c);
        for i in 0..3 {
            assert!((got[i] - expected[i]).abs() <= 1, "got {got:?}, expected {expected:?}");
        }
    }

    #[test]
    fn single_ramp_follows_the_v01_step_formula() {
        let r = Ramp::single(Rgba::hex("f3a2a6").unwrap(), 5, DEFAULT_HUE_STEP);
        assert_eq!(r.base, 2);
        close(r.stops[0], [148, 90, 122]);
        close(r.stops[1], [196, 124, 145]);
        close(r.stops[2], [243, 162, 166]);
        close(r.stops[3], [255, 194, 187]);
        close(r.stops[4], [255, 230, 223]);
    }

    #[test]
    fn line_matches_chibi_ramp_line() {
        close(line(Rgba::hex("f3a2a6").unwrap()), [106, 33, 70]);
    }

    #[test]
    fn fractional_shade_sits_between_neighbouring_stops() {
        let r = Ramp::single(Rgba::hex("f3a2a6").unwrap(), 5, DEFAULT_HUE_STEP);
        close(r.at(1.5), [219, 143, 156]);
    }

    #[test]
    fn mix_interpolates_in_oklab() {
        let m = mix(Rgba::hex("ff0000").unwrap(), Rgba::hex("0000ff").unwrap(), 0.5);
        close(m, [140, 83, 162]);
    }

    #[test]
    fn transparent_gradient_end_keeps_the_other_ends_color() {
        let coral = Rgba::hex("ec7a6a").unwrap();
        let stops = densify(&[(coral, None), (coral.with_alpha(0.0), None)]);
        assert_eq!(stops.len(), 17);
        let mid = stops[8].0;
        assert_eq!(bytes(mid), [236, 122, 106, 128]);
        assert_eq!(stops[8].1, 0.5);
    }

    #[test]
    fn stop_positions_fill_in_between_known_ones() {
        let c = Rgba::GRAY;
        let stops = densify(&[(c, None), (c, Some(0.3)), (c, None), (c, None)]);
        let knots: Vec<f64> = stops.iter().step_by(16).map(|s| (s.1 * 1000.0).round() / 1000.0).collect();
        assert_eq!(knots, vec![0.0, 0.3, 0.65, 1.0]);
    }
}
