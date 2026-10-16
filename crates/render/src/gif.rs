//! Deterministic GIF. One NeuQuant palette for the whole clip. Delays are already centiseconds.

use std::io::Cursor;

use artscript_ir::Scene;
use gif::{Encoder, Frame, Repeat};

use crate::RenderError;
use crate::draw;

pub fn encode(
    scenes: &[&Scene],
    scale: f32,
    delays: &[u16],
    loop_forever: bool,
) -> Result<Vec<u8>, RenderError> {
    if scenes.is_empty() {
        return Err(RenderError::Skia("gif has no frames".into()));
    }
    let bg = scenes[0].background.ok_or(RenderError::TransparentGif)?;
    if bg.a < 1.0 - 1e-4 {
        return Err(RenderError::TransparentGif);
    }
    let rendered: Result<Vec<(u32, u32, Vec<u8>)>, RenderError> = scenes
        .iter()
        .map(|scene| {
            let (fw, fh, px) = draw::render_rgba(scene, scale)?;
            if !opaque(&px) {
                return Err(RenderError::TransparentGif);
            }
            Ok((fw, fh, px))
        })
        .collect();
    let rendered = rendered?;
    let Some((w, h, _)) = rendered.first() else {
        return Err(RenderError::Skia("gif has no frames".into()));
    };
    let (w, h) = (*w, *h);
    if rendered.iter().any(|(fw, fh, _)| *fw != w || *fh != h) {
        return Err(RenderError::Skia("gif frames differ in size".into()));
    }
    if w > u16::MAX as u32 || h > u16::MAX as u32 {
        return Err(RenderError::Skia("gif is larger than 65535px".into()));
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
