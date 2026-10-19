//! Signed distance of a path over a device-pixel grid, as an F16 image a brush shader samples.
//! The path is rasterized with antialiasing on the CPU, then an exact two-pass Euclidean distance
//! transform runs outside and inside. CPU only, so both backends read the same texels. The cost
//! is the grid, which is the node's device bounds plus its reach, never the canvas.

use std::collections::HashMap;

use skia_safe::{AlphaType, ColorType, Data, IRect, ISize, Image, ImageInfo, Matrix, Paint, Path, images, surfaces};

use crate::RenderError;

/// Identity of one field: the same path, placement and grid give the same texels.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) struct SdfKey {
    path: Vec<u8>,
    matrix: [u32; 9],
    rect: (i32, i32, i32, i32),
}

impl SdfKey {
    pub(crate) fn new(path: &Path, ltd: &Matrix, rect: IRect) -> SdfKey {
        let m = [
            ltd.scale_x(), ltd.skew_x(), ltd.translate_x(),
            ltd.skew_y(), ltd.scale_y(), ltd.translate_y(),
            ltd.persp_x(), ltd.persp_y(), ltd[8],
        ];
        SdfKey {
            path: path.serialize().as_bytes().to_vec(),
            matrix: m.map(f32::to_bits),
            rect: (rect.left, rect.top, rect.right, rect.bottom),
        }
    }
}

#[derive(Default)]
pub(crate) struct SdfMemo {
    fields: HashMap<SdfKey, Image>,
}

impl SdfMemo {
    /// The field for `path` drawn through `ltd` over the device `rect`. A hit returns the image a
    /// miss would have built.
    pub(crate) fn get(&mut self, path: &Path, ltd: &Matrix, rect: IRect) -> Result<Image, RenderError> {
        let key = SdfKey::new(path, ltd, rect);
        if let Some(img) = self.fields.get(&key) {
            return Ok(img.clone());
        }
        let img = signed_distance(path, ltd, rect)?;
        self.fields.insert(key, img.clone());
        Ok(img)
    }
}

/// Texel `(i, j)` holds the signed distance in device pixels, + inside, at the center of device
/// pixel `(rect.left + i, rect.top + j)`. An edge pixel holds `coverage - 0.5`.
fn signed_distance(path: &Path, ltd: &Matrix, rect: IRect) -> Result<Image, RenderError> {
    let (w, h) = (rect.width().max(1) as usize, rect.height().max(1) as usize);
    let info = ImageInfo::new(ISize::new(w as i32, h as i32), ColorType::Alpha8, AlphaType::Premul, None);
    let mut surface = surfaces::raster(&info, None, None).ok_or_else(|| RenderError::Skia("sdf surface".into()))?;
    let canvas = surface.canvas();
    canvas.translate((-(rect.left as f32), -(rect.top as f32)));
    canvas.concat(ltd);
    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    canvas.draw_path(path, &paint);
    let mut cov = vec![0u8; w * h];
    if !surface.read_pixels(&info, &mut cov, w, (0, 0)) {
        return Err(RenderError::Skia("sdf read".into()));
    }
    const INF: f64 = 1e20;
    let inside: Vec<bool> = cov.iter().map(|c| *c >= 128).collect();
    let d_out = edt(&inside.iter().map(|b| if *b { 0.0 } else { INF }).collect::<Vec<_>>(), w, h);
    let d_in = edt(&inside.iter().map(|b| if *b { INF } else { 0.0 }).collect::<Vec<_>>(), w, h);
    let mut bytes = Vec::with_capacity(w * h * 8);
    for i in 0..w * h {
        let c = cov[i];
        let d = if c > 0 && c < 255 {
            c as f32 / 255.0 - 0.5
        } else if inside[i] {
            d_in[i].sqrt() as f32 - 0.5
        } else {
            -(d_out[i].sqrt() as f32 - 0.5)
        };
        for v in [d, 0.0, 0.0, 1.0] {
            bytes.extend_from_slice(&f16_bits(v).to_ne_bytes());
        }
    }
    // Half floats: the GPU backend cannot sample RGBA F32 textures, and 8 bits would clamp the
    // distance to 0..1.
    let info = ImageInfo::new(ISize::new(w as i32, h as i32), ColorType::RGBAF16, AlphaType::Premul, None);
    images::raster_from_data(&info, Data::new_copy(&bytes), w * 8).ok_or_else(|| RenderError::Skia("sdf image".into()))
}

/// IEEE half, round to nearest even, saturating at the largest finite half.
pub(crate) fn f16_bits(x: f32) -> u16 {
    let b = x.to_bits();
    let sign = ((b >> 16) & 0x8000) as u16;
    let exp = ((b >> 23) & 0xff) as i32 - 127 + 15;
    let man = b & 0x7f_ffff;
    if exp <= 0 {
        if exp < -10 {
            return sign;
        }
        let m = (man | 0x80_0000) >> (14 - exp);
        return sign | ((m + 0x1000) >> 13) as u16;
    }
    if exp >= 31 {
        return sign | 0x7bff;
    }
    let r = ((exp as u32) << 10) + ((man + 0x1000) >> 13);
    sign | r.min(0x7bff) as u16
}

#[cfg(test)]
pub(crate) fn half_to_f32(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = ((h >> 10) & 0x1f) as i32;
    let man = (h & 0x3ff) as f32;
    if exp == 0 {
        sign * man * 2f32.powi(-24)
    } else {
        sign * (1.0 + man / 1024.0) * 2f32.powi(exp - 15)
    }
}

/// Felzenszwalb-Huttenlocher squared Euclidean distance transform, columns then rows.
fn edt(f: &[f64], w: usize, h: usize) -> Vec<f64> {
    let mut out = f.to_vec();
    let mut col = vec![0.0; h];
    for x in 0..w {
        for y in 0..h {
            col[y] = out[y * w + x];
        }
        let d = edt1(&col);
        for y in 0..h {
            out[y * w + x] = d[y];
        }
    }
    for y in 0..h {
        let d = edt1(&out[y * w..y * w + w]);
        out[y * w..y * w + w].copy_from_slice(&d);
    }
    out
}

fn edt1(f: &[f64]) -> Vec<f64> {
    let n = f.len();
    let mut d = vec![0.0; n];
    let mut v = vec![0usize; n];
    let mut z = vec![0.0f64; n + 1];
    let mut k = 0usize;
    z[0] = f64::NEG_INFINITY;
    z[1] = f64::INFINITY;
    let sq = |i: usize| (i * i) as f64;
    for q in 1..n {
        let mut s = ((f[q] + sq(q)) - (f[v[k]] + sq(v[k]))) / (2.0 * (q - v[k]) as f64);
        while s <= z[k] {
            k -= 1;
            s = ((f[q] + sq(q)) - (f[v[k]] + sq(v[k]))) / (2.0 * (q - v[k]) as f64);
        }
        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = f64::INFINITY;
    }
    k = 0;
    for (q, out) in d.iter_mut().enumerate() {
        while z[k + 1] < q as f64 {
            k += 1;
        }
        *out = (q as f64 - v[k] as f64).powi(2) + f[v[k]];
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_floats_round_trip_small_integers_and_saturate() {
        for v in [0.0_f32, 1.0, -1.0, 0.5, 300.0, -17.25] {
            let bits = f16_bits(v);
            let back = half_to_f32(bits);
            assert_eq!(back, v, "{v}");
        }
        assert_eq!(f16_bits(1e9), 0x7bff);
        assert_eq!(f16_bits(-1e9), 0xfbff);
    }

    #[test]
    fn the_distance_is_signed_and_grows_away_from_the_edge() {
        let path = Path::rect(skia_safe::Rect::from_xywh(10.0, 10.0, 20.0, 20.0), None);
        let rect = IRect::from_ltrb(0, 0, 40, 40);
        let key_a = SdfKey::new(&path, &Matrix::new_identity(), rect);
        let key_b = SdfKey::new(&path, &Matrix::new_identity(), rect);
        assert!(key_a == key_b);
        let img = signed_distance(&path, &Matrix::new_identity(), rect).unwrap();
        let info = ImageInfo::new(ISize::new(40, 40), ColorType::RGBAF16, AlphaType::Premul, None);
        let mut buf = vec![0u8; 40 * 40 * 8];
        assert!(img.read_pixels(&info, &mut buf, 40 * 8, (0, 0), skia_safe::image::CachingHint::Disallow));
        let at = |x: usize, y: usize| half_to_f32(u16::from_ne_bytes([buf[(y * 40 + x) * 8], buf[(y * 40 + x) * 8 + 1]]));
        assert_eq!(at(20, 20), 9.5, "center of a 20px square is 9.5px inside");
        assert_eq!(at(5, 20), -4.5, "5 px left of the edge");
        assert_eq!(at(20, 2), -7.5);
        assert!(at(10, 20) > -0.6 && at(10, 20) < 0.6, "edge pixel holds coverage - .5");
    }
}
