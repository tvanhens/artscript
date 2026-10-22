use crate::{Matrix, Rect};

/// A positive, finite width and height, in artboard units or output units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Size {
    w: f32,
    h: f32,
}

impl Size {
    pub fn new(w: f32, h: f32) -> Option<Size> {
        (w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0).then_some(Size { w, h })
    }

    /// The size of `rect`, or `None` when it is empty or not finite.
    pub fn of(rect: Rect) -> Option<Size> {
        Size::new(rect.right - rect.left, rect.bottom - rect.top)
    }

    pub fn w(self) -> f32 {
        self.w
    }

    pub fn h(self) -> f32 {
        self.h
    }

    pub fn rect(self) -> Rect {
        Rect::new(0.0, 0.0, self.w, self.h)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelSize {
    pub w: u32,
    pub h: u32,
}

/// How the shown rect fills the output: one uniform zoom, never a stretch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Fit {
    /// The whole rect shows, as large as fits.
    #[default]
    Contain,
    /// The rect fills the output and is cropped.
    Cover,
    /// One scene unit per output unit, cropped or padded.
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Edge {
    Start,
    #[default]
    Center,
    End,
}

/// Where the shown rect sits when it and the output differ. `align:left,top` is
/// `Align { x: Start, y: Start }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Align {
    pub x: Edge,
    pub y: Edge,
}

/// One output's frame: its size in output units, its pixel density, and how a shown rect lands
/// in it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Framing {
    /// Output units, from the `@target` size or else the artboard size.
    pub out: Size,
    /// Pixels per output unit, one number from `scale:`. An SVG framing has 1.
    pub scale: f32,
    pub fit: Fit,
    pub align: Align,
}

impl Framing {
    /// A frame the size of `out` at `scale`. Its view of a rect that size is a plain scale.
    pub fn plain(out: Size, scale: f32) -> Framing {
        Framing {
            out,
            scale,
            fit: Fit::default(),
            align: Align::default(),
        }
    }

    /// The image size: `ceil(out * scale)`, at least 1, per side.
    pub fn pixels(&self) -> PixelSize {
        let side = |units: f32| (units * self.scale).ceil().max(1.0) as u32;
        PixelSize {
            w: side(self.out.w),
            h: side(self.out.h),
        }
    }

    /// Maps `source`, in scene units, into this frame. The zoom is the fit factor of
    /// `out / source` times `scale`, and the offset puts the aligned rect on a whole pixel. When
    /// `out` is the size of `source`, the zoom is exactly `scale` and the offset exactly 0.
    pub fn view(&self, source: Rect) -> View {
        let (sw, sh) = (source.right - source.left, source.bottom - source.top);
        let fit = match self.fit {
            Fit::Contain => (self.out.w / sw).min(self.out.h / sh),
            Fit::Cover => (self.out.w / sw).max(self.out.h / sh),
            Fit::None => 1.0,
        };
        let zoom = fit * self.scale;
        let along = |edge: Edge, out: f32, shown: f32| {
            let slack = out * self.scale - shown * zoom;
            match edge {
                Edge::Start => 0.0,
                Edge::Center => (slack * 0.5).round(),
                Edge::End => slack.round(),
            }
        };
        View {
            pixels: self.pixels(),
            source,
            zoom,
            offset: (
                along(self.align.x, self.out.w, sw) - source.left * zoom,
                along(self.align.y, self.out.h, sh) - source.top * zoom,
            ),
        }
    }
}

/// The scene-to-pixel map of one rendered frame: a uniform zoom and a whole-pixel translation,
/// so it is axis-aligned and invertible.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub pixels: PixelSize,
    /// Scene units shown. Ink outside it is clipped.
    pub source: Rect,
    /// Output pixels per scene unit.
    pub zoom: f32,
    /// Pixel position of scene `(0, 0)`.
    pub offset: (f32, f32),
}

impl View {
    pub fn to_px(&self, (x, y): (f64, f64)) -> (f64, f64) {
        let zoom = f64::from(self.zoom);
        (
            x * zoom + f64::from(self.offset.0),
            y * zoom + f64::from(self.offset.1),
        )
    }

    pub fn rect_to_px(&self, r: Rect) -> Rect {
        Rect::new(
            r.left * self.zoom + self.offset.0,
            r.top * self.zoom + self.offset.1,
            r.right * self.zoom + self.offset.0,
            r.bottom * self.zoom + self.offset.1,
        )
    }

    /// The scene rect the pixel rect `r` shows.
    pub fn rect_from_px(&self, r: Rect) -> Rect {
        Rect::new(
            (r.left - self.offset.0) / self.zoom,
            (r.top - self.offset.1) / self.zoom,
            (r.right - self.offset.0) / self.zoom,
            (r.bottom - self.offset.1) / self.zoom,
        )
    }

    /// The scene rect the whole image shows.
    pub fn visible(&self) -> Rect {
        self.rect_from_px(Rect::new(0.0, 0.0, self.pixels.w as f32, self.pixels.h as f32))
    }

    pub fn matrix(&self) -> Matrix {
        Matrix([self.zoom, 0.0, self.offset.0, 0.0, self.zoom, self.offset.1, 0.0, 0.0, 1.0])
    }

    /// The pixel rect of `source`, rounded to whole pixels so a non-antialiased clip is exact,
    /// or `None` when it covers the whole image and a clip could not change a pixel.
    pub fn clip(&self) -> Option<Rect> {
        let r = self.rect_to_px(self.source);
        let r = Rect::new(
            r.left.round(),
            r.top.round(),
            r.right.round(),
            r.bottom.round(),
        );
        let covers = r.left <= 0.0
            && r.top <= 0.0
            && r.right >= self.pixels.w as f32
            && r.bottom >= self.pixels.h as f32;
        (!covers).then_some(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn framing(w: f32, h: f32, scale: f32, fit: Fit, x: Edge, y: Edge) -> Framing {
        Framing {
            out: Size::new(w, h).unwrap(),
            scale,
            fit,
            align: Align { x, y },
        }
    }

    const ART: Rect = Rect {
        left: 0.0,
        top: 0.0,
        right: 64.0,
        bottom: 32.0,
    };

    #[test]
    fn a_size_is_positive_and_finite() {
        assert!(Size::new(1.0, 0.5).is_some());
        assert!(Size::new(0.0, 1.0).is_none());
        assert!(Size::new(1.0, -1.0).is_none());
        assert!(Size::new(f32::NAN, 1.0).is_none());
        assert!(Size::new(1.0, f32::INFINITY).is_none());
    }

    #[test]
    fn pixels_round_up_and_never_reach_zero() {
        assert_eq!(
            Framing::plain(Size::new(33.0, 20.0).unwrap(), 0.5).pixels(),
            PixelSize { w: 17, h: 10 }
        );
        assert_eq!(
            Framing::plain(Size::new(1.0, 1.0).unwrap(), 0.1).pixels(),
            PixelSize { w: 1, h: 1 }
        );
    }

    #[test]
    fn an_output_the_size_of_the_source_is_a_plain_scale() {
        for scale in [0.5, 1.0, 2.0, 2.5] {
            for fit in [Fit::Contain, Fit::Cover, Fit::None] {
                let v = framing(64.0, 32.0, scale, fit, Edge::End, Edge::Start).view(ART);
                assert_eq!(v.zoom, scale, "{fit:?} {scale}");
                assert_eq!(v.offset, (0.0, 0.0), "{fit:?} {scale}");
                assert_eq!(v.clip(), None, "{fit:?} {scale}");
            }
        }
    }

    #[test]
    fn contain_letterboxes_and_aligns() {
        let v = framing(48.0, 48.0, 1.0, Fit::Contain, Edge::Center, Edge::Center).view(ART);
        assert_eq!((v.zoom, v.offset), (0.75, (0.0, 12.0)));
        assert_eq!(v.clip(), Some(Rect::new(0.0, 12.0, 48.0, 36.0)));
        assert_eq!(v.to_px((32.0, 32.0)), (24.0, 36.0));
        let top = framing(48.0, 48.0, 1.0, Fit::Contain, Edge::Center, Edge::Start).view(ART);
        assert_eq!(top.offset, (0.0, 0.0));
        let bottom = framing(48.0, 48.0, 1.0, Fit::Contain, Edge::Center, Edge::End).view(ART);
        assert_eq!(bottom.offset, (0.0, 24.0));
        let wide = framing(96.0, 32.0, 1.0, Fit::Contain, Edge::End, Edge::Center).view(ART);
        assert_eq!((wide.zoom, wide.offset), (1.0, (32.0, 0.0)));
    }

    #[test]
    fn contain_scales_with_the_density() {
        let v = framing(48.0, 48.0, 2.0, Fit::Contain, Edge::Center, Edge::Center).view(ART);
        assert_eq!((v.zoom, v.offset), (1.5, (0.0, 24.0)));
        assert_eq!(v.pixels, PixelSize { w: 96, h: 96 });
    }

    #[test]
    fn cover_crops_and_aligns() {
        let v = framing(48.0, 48.0, 1.0, Fit::Cover, Edge::Center, Edge::Center).view(ART);
        assert_eq!((v.zoom, v.offset), (1.5, (-24.0, 0.0)));
        assert_eq!(v.clip(), None);
        let left = framing(48.0, 48.0, 1.0, Fit::Cover, Edge::Start, Edge::Center).view(ART);
        assert_eq!(left.offset, (0.0, 0.0));
        let right = framing(48.0, 48.0, 1.0, Fit::Cover, Edge::End, Edge::Center).view(ART);
        assert_eq!(right.offset, (-48.0, 0.0));
    }

    #[test]
    fn none_keeps_one_unit_per_output_unit() {
        let v = framing(96.0, 48.0, 1.0, Fit::None, Edge::End, Edge::End).view(ART);
        assert_eq!((v.zoom, v.offset), (1.0, (32.0, 16.0)));
        assert_eq!(v.clip(), Some(Rect::new(32.0, 16.0, 96.0, 48.0)));
        let small = framing(32.0, 32.0, 2.0, Fit::None, Edge::Center, Edge::Center).view(ART);
        assert_eq!((small.zoom, small.offset), (2.0, (-32.0, 0.0)));
    }

    #[test]
    fn the_offset_lands_on_a_whole_pixel() {
        let v = framing(50.0, 50.0, 1.0, Fit::Contain, Edge::Center, Edge::Center).view(ART);
        assert_eq!(v.zoom, 50.0 / 64.0);
        assert_eq!(v.offset, (0.0, 13.0));
    }

    #[test]
    fn a_source_away_from_the_origin_is_moved_to_the_frame() {
        let shown = Rect::new(16.0, 8.0, 48.0, 24.0);
        let v = framing(32.0, 16.0, 1.0, Fit::Contain, Edge::Center, Edge::Center).view(shown);
        assert_eq!((v.zoom, v.offset), (1.0, (-16.0, -8.0)));
        assert_eq!(v.to_px((16.0, 8.0)), (0.0, 0.0));
        assert_eq!(v.visible(), shown);
    }
}
