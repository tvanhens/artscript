//! Pictures read from the project folder: `image("path")` paints with one and `sample("path" x,y)`
//! reads a color from it. Each file is decoded once per process and shared.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::SystemTime;

use skia_safe::image::CachingHint;
use skia_safe::{AlphaType, ColorType, Data, Image, ImageInfo};

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
    #[allow(dead_code)]
    rgba: Vec<u8>,
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
