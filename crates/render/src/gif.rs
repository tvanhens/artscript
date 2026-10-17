use std::io::Cursor;
use std::sync::{Condvar, Mutex};

use artscript_ir::Scene;
use gif::{Encoder, Frame, Repeat};

use crate::Backend;
use crate::draw;
use crate::{RASTER_BUDGET, RenderError};

static GIF_HELD: Mutex<u64> = Mutex::new(0);
static GIF_SLOT: Condvar = Condvar::new();

/// `held` is RGBA bytes already reserved by other encodes. A lone raster may use the whole cap.
fn raster_slot_open(held: u64, bytes: u64) -> bool {
    held == 0 || held.saturating_add(bytes) <= RASTER_BUDGET
}

struct GifSlot {
    bytes: u64,
}

impl Drop for GifSlot {
    fn drop(&mut self) {
        let mut held = GIF_HELD.lock().unwrap_or_else(|e| e.into_inner());
        *held = held.saturating_sub(self.bytes);
        GIF_SLOT.notify_all();
    }
}

fn reserve_raster(bytes: u64) -> GifSlot {
    let mut held = GIF_HELD.lock().unwrap_or_else(|e| e.into_inner());
    while !raster_slot_open(*held, bytes) {
        held = GIF_SLOT.wait(held).unwrap_or_else(|e| e.into_inner());
    }
    *held = held.saturating_add(bytes);
    GifSlot { bytes }
}

pub fn encode(
    scenes: &[&Scene],
    scale: f32,
    delays: &[u16],
    loop_forever: bool,
    backend: Backend,
) -> Result<Vec<u8>, RenderError> {
    if scenes.is_empty() {
        return Err(RenderError::Skia("gif has no frames".into()));
    }
    let bg = scenes[0].background.ok_or(RenderError::TransparentGif)?;
    if bg.a < 1.0 - 1e-4 {
        return Err(RenderError::TransparentGif);
    }
    if !(scale.is_finite() && scale > 0.0) {
        return Err(RenderError::Skia("scale must be positive".into()));
    }
    let mut bytes = 0u64;
    let mut dims = Vec::with_capacity(scenes.len());
    for scene in scenes {
        let w = (scene.width * scale).ceil().max(1.0);
        let h = (scene.height * scale).ceil().max(1.0);
        if !w.is_finite() || !h.is_finite() {
            return Err(RenderError::Skia("gif frame has no size".into()));
        }
        let (w, h) = (w as u32, h as u32);
        if w > u16::MAX as u32 || h > u16::MAX as u32 {
            return Err(RenderError::Skia("gif is larger than 65535px".into()));
        }
        bytes = bytes.saturating_add(u64::from(w).saturating_mul(u64::from(h)).saturating_mul(4));
        dims.push((w, h));
    }
    let (w, h) = dims[0];
    if dims.iter().any(|(fw, fh)| *fw != w || *fh != h) {
        return Err(RenderError::Skia("gif frames differ in size".into()));
    }
    if bytes > RASTER_BUDGET {
        return Err(RenderError::GifTooLarge);
    }
    let _slot = reserve_raster(bytes);
    let rendered: Result<Vec<(u32, u32, Vec<u8>)>, RenderError> = scenes
        .iter()
        .map(|scene| {
            let (fw, fh, px) = draw::render_rgba(scene, scale, backend)?;
            if !opaque(&px) {
                return Err(RenderError::TransparentGif);
            }
            Ok((fw, fh, px))
        })
        .collect();
    let rendered = rendered?;
    if rendered.iter().any(|(fw, fh, _)| *fw != w || *fh != h) {
        return Err(RenderError::Skia("gif frames differ in size".into()));
    }
    let rasters: Vec<&[u8]> = rendered.iter().map(|(_, _, px)| px.as_slice()).collect();
    let flat: Vec<u8> = rasters.iter().flat_map(|px| px.iter().copied()).collect();
    let nq = color_quant::NeuQuant::new(10, 256, &flat);
    let palette = rgb_palette(&nq.color_map_rgba());
    let mut buf = Cursor::new(Vec::new());
    let mut encoder = Encoder::new(&mut buf, w as u16, h as u16, &palette)
        .map_err(|e| RenderError::Skia(e.to_string()))?;
    if loop_forever {
        encoder
            .set_repeat(Repeat::Infinite)
            .map_err(|e| RenderError::Skia(e.to_string()))?;
    }
    for (i, px) in rasters.iter().enumerate() {
        let mut index = vec![0u8; (w * h) as usize];
        for (p, idx) in px.as_chunks::<4>().0.iter().zip(index.iter_mut()) {
            *idx = nq.index_of(p) as u8;
        }
        let mut frame = Frame::from_indexed_pixels(w as u16, h as u16, index, None);
        frame.delay = delays.get(i).copied().unwrap_or(4);
        encoder
            .write_frame(&frame)
            .map_err(|e| RenderError::Skia(e.to_string()))?;
    }
    drop(encoder);
    Ok(buf.into_inner())
}

fn rgb_palette(rgba: &[u8]) -> Vec<u8> {
    rgba.chunks(4).flat_map(|p| [p[0], p[1], p[2]]).collect()
}

fn opaque(px: &[u8]) -> bool {
    px.as_chunks::<4>().0.iter().all(|p| p[3] == 255)
}

#[cfg(test)]
mod tests {
    use artscript_ir::{Color, Scene};

    use super::*;

    fn blank(size: f32) -> Scene {
        Scene {
            width: size,
            height: size,
            background: Some(Color {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 1.0,
            }),
            children: Vec::new(),
        }
    }

    #[test]
    fn a_raster_over_the_budget_is_rejected_before_render() {
        let scene = blank(4096.0);
        let frames = vec![&scene; 5];
        let err = encode(&frames, 1.0, &[4; 5], false, Backend::Cpu).unwrap_err();
        assert!(matches!(err, RenderError::GifTooLarge));
    }

    #[test]
    fn parallel_targets_share_one_raster_cap() {
        let cap = RASTER_BUDGET;
        assert!(raster_slot_open(0, cap));
        assert!(!raster_slot_open(cap, 1));
        assert!(raster_slot_open(cap / 2, cap / 2));
        assert!(!raster_slot_open(cap / 2, cap / 2 + 1));
    }

    #[test]
    fn a_small_gif_still_encodes() {
        let scene = blank(8.0);
        let bytes = encode(&[&scene], 1.0, &[4], false, Backend::Cpu).unwrap();
        assert!(bytes.starts_with(b"GIF"));
    }
}
