//! Pictures read from the project folder: `image("path")` paints with one and `sample("path" x,y)`
//! reads a color from it. Each file is decoded once per process and shared.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::SystemTime;

use skia_safe::image::CachingHint;
use skia_safe::{AlphaType, ColorType, Data, Image, ImageInfo};

use crate::color::{Rgba, to_linear, to_srgb};

/// How `image(...)` fills the shape's bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    /// The whole picture, stretched to the bounds.
    Stretch,
    /// The picture keeps its aspect and covers the bounds, cropped about the center.
    Cover,
}

/// An `image(...)` paint value.
#[derive(Clone)]
pub struct ImagePaint {
    pub decoded: Rc<Decoded>,
    pub fit: Fit,
}

/// A decoded picture: the skia image for drawing and its pixels for `sample`.
pub struct Decoded {
    pub image: Image,
    pub width: i32,
    pub height: i32,
    /// Row-major RGBA, 8 bits per channel, not premultiplied.
    rgba: Vec<u8>,
}

impl Decoded {
    /// The color at pixel `x`,`y` (origin top-left), averaged over the box of `radius` pixels
    /// around it that lies inside the picture, in linear light. `None` when `x`,`y` is outside.
    pub fn sample(&self, x: f64, y: f64, radius: i32) -> Option<Rgba> {
        let (cx, cy) = (x.floor() as i64, y.floor() as i64);
        if cx < 0 || cy < 0 || cx >= self.width as i64 || cy >= self.height as i64 {
            return None;
        }
        let r = i64::from(radius.max(0));
        let (x0, x1) = ((cx - r).max(0), (cx + r).min(self.width as i64 - 1));
        let (y0, y1) = ((cy - r).max(0), (cy + r).min(self.height as i64 - 1));
        let mut sum = [0.0_f64; 3];
        for py in y0..=y1 {
            for px in x0..=x1 {
                let at = (py as usize * self.width as usize + px as usize) * 4;
                for (s, byte) in sum.iter_mut().zip(&self.rgba[at..at + 3]) {
                    *s += to_linear(f64::from(*byte) / 255.0);
                }
            }
        }
        let n = ((x1 - x0 + 1) * (y1 - y0 + 1)) as f64;
        let [r, g, b] = sum.map(|s| (to_srgb(s / n) * 255.0).round() / 255.0);
        Some(Rgba { r, g, b, a: 1.0 })
    }
}

type Stamp = (SystemTime, u64);

thread_local! {
    static CACHE: RefCell<HashMap<PathBuf, (Stamp, Rc<Decoded>)>> = RefCell::new(HashMap::new());
}

/// The picture at `rel` under the project `root`. A file that changed on disk is decoded again.
pub fn load(root: &Path, rel: &str) -> Result<Rc<Decoded>, String> {
    let path = root.join(rel);
    let meta = std::fs::metadata(&path).map_err(|e| format!("cannot read image \"{rel}\": {e}"))?;
    let stamp = (meta.modified().unwrap_or(SystemTime::UNIX_EPOCH), meta.len());
    if let Some(hit) = CACHE.with(|c| c.borrow().get(&path).filter(|(s, _)| *s == stamp).map(|(_, d)| d.clone())) {
        return Ok(hit);
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("cannot read image \"{rel}\": {e}"))?;
    let decoded = Rc::new(decode(&bytes).ok_or_else(|| format!("cannot decode image \"{rel}\" as PNG or JPEG"))?);
    CACHE.with(|c| c.borrow_mut().insert(path, (stamp, decoded.clone())));
    Ok(decoded)
}

fn decode(bytes: &[u8]) -> Option<Decoded> {
    let encoded = Image::from_encoded(Data::new_copy(bytes))?;
    let image = encoded.make_raster_image(None, CachingHint::Allow)?;
    let (width, height) = (image.width(), image.height());
    let info = ImageInfo::new((width, height), ColorType::RGBA8888, AlphaType::Unpremul, None);
    let mut rgba = vec![0u8; width as usize * height as usize * 4];
    image
        .read_pixels(&info, rgba.as_mut_slice(), width as usize * 4, (0, 0), CachingHint::Allow)
        .then_some(Decoded { image, width, height, rgba })
}
