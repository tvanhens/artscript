//! Scene IR: a fully resolved tree that every backend reads.
//!
//! All numbers are absolute canvas units. Geometry is final (booleans, splines, tapered strokes
//! and jitter already applied). Gradients carry dense sRGB stops already interpolated in OKLCH,
//! so backends interpolate linearly between neighbouring stops and still match each other.

use std::sync::Arc;

pub use ramose_brush::UniformValue;

mod geometry;
mod retained;
pub mod warp;

pub use geometry::{FillType, Matrix, Path, Point, Primitive, Rect, Verb};
pub use warp::{Axis, Corners, CornersError, Cylinder, Warp, WarpStage};

/// sRGB, 0..1, straight (not premultiplied) alpha.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const TRANSPARENT: Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };

    pub fn hex(&self) -> String {
        let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        if self.a >= 1.0 {
            format!("#{:02x}{:02x}{:02x}", c(self.r), c(self.g), c(self.b))
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", c(self.r), c(self.g), c(self.b), c(self.a))
        }
    }
}

/// The sides of a node's local box an effect draws on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sides {
    pub top: bool,
    pub right: bool,
    pub bottom: bool,
    pub left: bool,
}

impl Sides {
    pub const ALL: Sides = Sides { top: true, right: true, bottom: true, left: true };

    pub fn is_all(self) -> bool {
        self == Sides::ALL
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    /// Pure SVG; any raster-only feature is an error.
    Vector,
    /// SVG, raster-only subtrees baked to embedded PNG tiles.
    Svg,
    /// Skia renders directly.
    Raster,
}

/// The pixel size of a `width` x `height` canvas drawn at `scale`: a PNG's size.
pub fn pixel_size(width: f32, height: f32, scale: f32) -> (i32, i32) {
    let side = |canvas: f32| (canvas * scale).ceil().max(1.0) as i32;
    (side(width), side(height))
}

#[derive(Clone, Debug)]
pub struct Scene {
    pub width: f32,
    pub height: f32,
    pub background: Option<Color>,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug)]
pub struct Node {
    /// Stable dotted path, e.g. `hero.body.arm_r`. An unnamed node gets `verb~N`, its count among
    /// the draws with that verb in its block from 1, as in `body.eye.0.circle~1`.
    pub path: String,
    /// Source location `file:line:col` for diagnostics and `explain`.
    pub origin: String,
    /// Local to parent, applied after `warp`.
    pub transform: Matrix,
    /// Applied to everything this node draws, before `transform`. Children, clips, masks and
    /// stamps follow it; effects run after it; brushes and fields sample before it.
    pub warp: Warp,
    pub opacity: f32,
    pub blend: Blend,
    /// Clip in this node's local coordinates (from `inside @x` / `outside @x`).
    pub clip: Option<Clip>,
    pub mask: Option<Box<Mask>>,
    /// Applied to this node's composed content, in order; see [`Effect`].
    pub effects: Vec<Effect>,
    pub kind: NodeKind,
}

#[derive(Clone, Debug)]
pub enum NodeKind {
    /// `layer` and def instances set `isolate`: child blend modes never reach content below the group.
    Group { isolate: bool, children: Vec<Node> },
    Shape(Box<Shape>),
}

#[derive(Clone, Debug)]
pub struct Shape {
    pub geometry: Path,
    pub fill: Option<Fill>,
    pub stroke: Option<Stroke>,
    /// Gaussian sigma that softens the shape's own edge (Skia mask-filter blur). 0 = crisp.
    pub blur: f32,
    /// Drawn after the shape's own paint and before `children`, in the shape's local frame and
    /// not clipped to the geometry. Each is an isolated group placed at a point of the
    /// geometry's contours with x along the direction of travel. Stamps are the node's ink
    /// (effects, mask, opacity, blend and `fx outline` cover them) and not its geometry
    /// (children, `inside`, `copy`, booleans and anchors see the spine as before).
    pub stamps: Vec<Node>,
    /// Drawn after the shape, clipped to [`Shape::clip`] whatever the shape's fill or alpha (the
    /// "clip by nesting" rule), or to the coverage of a brushed fill. The shape, its stamps and
    /// its children form one isolated group, so a child's blend mode never reaches content below
    /// the shape.
    pub children: Vec<Node>,
}

/// A fill and the brush marks that shade it. An empty `marks` is a plain fill. With marks, the
/// stages run in order over the shape's signed distance field: stage 0 reads `paint` at each
/// pixel as `m.paint`, each later stage reads the previous stage's color, and coverages multiply.
#[derive(Clone, Debug)]
pub struct Fill {
    pub paint: Paint,
    pub marks: Vec<AreaMark>,
}

impl Fill {
    pub fn plain(paint: Paint) -> Fill {
        Fill { paint, marks: Vec::new() }
    }

    /// Local units the marks may paint past the geometry.
    pub fn reach(&self) -> f32 {
        self.marks.iter().map(|m| m.reach).fold(0.0, f32::max)
    }

    /// Whether a stage reads `m.below`, the pixels already drawn under the node.
    pub fn reads_below(&self) -> bool {
        self.marks.iter().any(|m| m.stage.program.reads_below())
    }
}

/// One brush stage over a fill. `m.uv`, `m.s`, `m.u` run along `dir_deg` across the geometry's
/// bounds rotated by that angle; `m.edge` is the signed distance in local units, + inside.
#[derive(Clone, Debug)]
pub struct AreaMark {
    pub stage: Stage,
    pub dir_deg: f32,
    /// `cos` and `sin` of `dir_deg`, computed on the CPU so both backends rotate alike.
    pub cos_sin: [f32; 2],
    /// Local units the stage may paint past the geometry. Grows bounds and bake tiles.
    pub reach: f32,
}

/// One compiled-checkable brush program with its uniform values and seed.
#[derive(Clone, Debug)]
pub struct Stage {
    /// The assembled program eval checked. Render compiles the same text.
    pub program: Arc<ramose_brush::Program>,
    /// Author uniforms in declaration order, by name.
    pub uniforms: Vec<(String, UniformValue)>,
    /// The site seed, reduced to 32 bits. Render derives the shader's seed uniforms from it.
    pub seed: u32,
}

#[derive(Clone, Debug)]
pub struct Stroke {
    pub paint: Paint,
    /// The widest the stroke gets. A ribbon's half width at each point is in its knots.
    pub width: f32,
    pub cap: Cap,
    pub join: Join,
    pub dash: Vec<f32>,
    /// The filled outline of a tapered or width-list stroke. When `Some`, the stroke's ink is
    /// this outline filled with `paint`, not the geometry stroked at `width`, and `cap`, `join`
    /// and `dash` do not apply. Outline effects, children and `inside` read it. `None` at
    /// constant width.
    pub ink: Option<Path>,
    /// The brush stages that shade the stroke as a ribbon along the shape's geometry. `None` is a
    /// plain stroke. A ribbon has round caps and joins and no dash.
    pub ribbon: Option<Ribbon>,
}

/// A brushed stroke. The spine is the shape's geometry; each of its contours is one ribbon with
/// its own channel knots. The stages run in order over the ribbon's `(s, d)` field: stage 0 reads
/// `paint` as `m.paint`, each later stage reads the previous stage's color, and coverages multiply.
#[derive(Clone, Debug)]
pub struct Ribbon {
    pub marks: Vec<RibbonMark>,
    /// One per contour of the geometry, in path order.
    pub contours: Vec<RibbonContour>,
    /// The fraction of each contour's `len` over which its start and its end thin, through
    /// [`taper_ramp`]. Each contour is its own pen stroke.
    pub taper: [f32; 2],
}

impl Ribbon {
    pub fn reach(&self) -> f32 {
        ribbon_reach(&self.marks)
    }

    /// Whether a stage reads `m.below`, the pixels already drawn under the node.
    pub fn reads_below(&self) -> bool {
        ribbon_reads_below(&self.marks)
    }
}

/// The farthest any of `marks` may paint past the ribbon's edge, in local units.
pub fn ribbon_reach(marks: &[RibbonMark]) -> f32 {
    marks.iter().map(|m| m.reach).fold(0.0, f32::max)
}

/// Whether a stage of `marks` reads `m.below`.
pub fn ribbon_reads_below(marks: &[RibbonMark]) -> bool {
    marks.iter().any(|m| m.stage.program.reads_below())
}

/// One brush stage over a stroke. `m.s` and `m.d` are the arc length and signed offset of the
/// point of the spine whose ink covers the pixel deepest, `m.edge` is the half width there
/// minus the distance to it.
#[derive(Clone, Debug)]
pub struct RibbonMark {
    pub stage: Stage,
    /// Local units the stage may paint past the ribbon's edge. Grows bounds and bake tiles.
    pub reach: f32,
}

/// The channel knots of one contour. Between knots every value interpolates linearly by `s`.
#[derive(Clone, Debug)]
pub struct RibbonContour {
    pub len: f32,
    pub closed: bool,
    /// Rising `s`, the first at 0 and the last at `len`.
    pub knots: Vec<Knot>,
}

impl RibbonContour {
    /// The channels at arc length `s`, clamped to the contour.
    pub fn at(&self, s: f32) -> Knot {
        let k = &self.knots;
        let i = k.partition_point(|n| n.s <= s);
        if i == 0 {
            return k[0];
        }
        if i == k.len() {
            return k[i - 1];
        }
        let (a, b) = (k[i - 1], k[i]);
        let t = ((s - a.s) / (b.s - a.s).max(1e-9)).clamp(0.0, 1.0);
        let mix = |x: f32, y: f32| x + (y - x) * t;
        Knot {
            s,
            half_width: mix(a.half_width, b.half_width),
            pressure: mix(a.pressure, b.pressure),
            angle: mix(a.angle, b.angle),
            tilt: mix(a.tilt, b.tilt),
            velocity: mix(a.velocity, b.velocity),
        }
    }
}

/// The ribbon's channels at one arc length. `half_width` is before the taper.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Knot {
    pub s: f32,
    pub half_width: f32,
    pub pressure: f32,
    pub angle: f32,
    pub tilt: f32,
    pub velocity: f32,
}

/// How much of its width a stroke keeps at fraction `u` of its length under `taper`, the start
/// and end fractions over which it thins. Shared by the tapered outline and the ribbon.
pub fn taper_ramp(u: f64, taper: (f64, f64)) -> f64 {
    const FLOOR: f64 = 0.12;
    let ease = |x: f64| (x.clamp(0.0, 1.0) * std::f64::consts::PI / 2.0).sin().powf(0.8);
    let mut w = 1.0;
    if taper.0 > 0.0 {
        w *= FLOOR + (1.0 - FLOOR) * ease(u / taper.0);
    }
    if taper.1 > 0.0 {
        w *= FLOOR + (1.0 - FLOOR) * ease((1.0 - u) / taper.1);
    }
    w
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cap {
    Round,
    Butt,
    Square,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Join {
    Round,
    Miter,
    Bevel,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stop {
    pub pos: f32,
    pub color: Color,
}

/// Gradient points are in the owning node's local coordinates.
#[derive(Clone, Debug)]
pub enum Paint {
    Solid(Color),
    Linear { from: Point, to: Point, stops: Vec<Stop> },
    Radial { center: Point, radius: f32, stops: Vec<Stop> },
    /// Sweeps clockwise from `start_deg` (0 = +x, 90 = +y, i.e. down).
    Conic { center: Point, start_deg: f32, stops: Vec<Stop> },
    /// A scalar field mapped through `stops` (field value 0..1 = stop position).
    Field { field: Field, stops: Vec<Stop> },
}

#[derive(Clone, Debug)]
pub struct Clip {
    pub path: Path,
    /// Gaussian sigma of a soft clip edge. 0 = hard anti-aliased clip.
    pub feather: f32,
    /// `outside @x`: keep everything except the path.
    pub invert: bool,
}

#[derive(Clone, Debug)]
pub struct Mask {
    pub source: MaskSource,
    /// Use luminance instead of alpha of the source.
    pub luminance: bool,
    pub feather: f32,
    pub invert: bool,
}

#[derive(Clone, Debug)]
pub enum MaskSource {
    /// Rendered in the masked node's local coordinates.
    Nodes(Vec<Node>),
    /// Continuous field (0..1 coverage), or thresholded when `threshold` is set (`field > .5`).
    Field { field: Field, threshold: Option<f32> },
}

/// Scalar field over the owning node's local coordinates, nominally 0..1.
#[derive(Clone, Debug, PartialEq)]
pub enum Field {
    /// `scale` is the feature size in canvas units. `wrap` is a period per axis in the same
    /// units: the field repeats every `wrap[0]` along x and every `wrap[1]` along y, with the
    /// feature size snapped so a whole number of cells fits each period.
    Noise { kind: NoiseKind, scale: f32, seed: u32, wrap: Option<[f32; 2]> },
    /// Fractal sum of `octaves` copies of `base` at doubling frequency and halving amplitude,
    /// renormalised to 0..1. `ridged` folds each octave as `1 - |2v - 1|`.
    Fbm { base: Box<Field>, octaves: u32, ridged: bool },
    Const(f32),
    Unary(FieldFn, Box<Field>),
    Binary(FieldOp, Box<Field>, Box<Field>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoiseKind {
    Simplex,
    Perlin,
    Worley,
    Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldFn {
    Abs,
    Neg,
    /// Clamp to 0..1.
    Clamp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldOp {
    Add,
    Sub,
    Mul,
    Div,
    Min,
    Max,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
    /// Additive (`blend:add`).
    Plus,
    /// Punches the node out of what is below it inside the enclosing isolated group (`blend:erase`).
    Erase,
}

/// One `fx` line. Effects run in order; each takes the previous result as input
/// (the node's own composed content for the first one) unless `input` names another source.
/// `name` (`as:`) publishes the result so later effects of the same node can use it.
#[derive(Clone, Debug)]
pub struct Effect {
    pub op: EffectOp,
    pub input: EffectInput,
    pub name: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EffectInput {
    /// The previous effect's result, or the node's content for the first effect.
    Previous,
    /// The node's own content, regardless of earlier effects (`self`).
    Source,
    /// An earlier effect's `as:` name on the same node.
    Named(String),
    /// Another node's rendered content, by node path, drawn in this node's coordinates.
    Node(String),
}

#[derive(Clone, Debug)]
pub enum EffectOp {
    /// Gaussian blur of the input.
    Blur { sigma: f32 },
    /// Input over a blurred, offset, recoloured copy of its alpha.
    Shadow { dx: f32, dy: f32, blur: f32, color: Color },
    /// Shadow cast inward from the edges, on top of the input and inside its alpha.
    /// Offset 0,6 darkens the top edge (CSS `inset` semantics).
    Inner { dx: f32, dy: f32, blur: f32, color: Color },
    /// Soft halo of `color` behind the input, `radius` sigma. `strength` multiplies the halo's
    /// alpha, clamped to 1. The blur kernel's support bounds the halo, so a strength above 1
    /// brightens it and reaches no further than a glow at full alpha.
    Glow { radius: f32, color: Color, strength: f32 },
    /// Thin light band just inside the edge on the side facing `dir` (screen space, y down),
    /// brightest at the edge and fading inward over `width`.
    Rim { width: f32, dir: Point, color: Color },
    /// OKLCH shift of every pixel: hue degrees, chroma fraction (+.2 = 20% more), OKLab lightness.
    Adjust { hue: f32, sat: f32, light: f32 },
    /// Line of `width` around the node's shape geometry, drawn behind the content.
    /// Earlier effects do not change the traced path. Ink past an edge of the traced
    /// geometry's box is dropped on every side `sides` leaves out. `mask` keeps the ring's
    /// ink only where it keeps it, in the node's local coordinates like the node's own mask,
    /// and its `feather` fades the ring's alpha at the mask edge. The content is untouched.
    /// `brush` shades the ring as a closed ribbon of half width `width` along each contour of
    /// the traced area, with the area on the ribbon's right; empty is a plain ring.
    Outline { width: f32, color: Color, sides: Sides, mask: Option<Box<Mask>>, brush: Vec<RibbonMark> },
    /// 3-octave Perlin inside the input's alpha, overlay-blended. `strength` is the brightness
    /// swing (.05 = 5%): the overlay opacity is `min(1, 2 * strength)`. `scale` is the finest
    /// feature in canvas units.
    Grain { strength: f32, scale: f32, seed: u32 },
    /// Composites `inputs` bottom to top, each with its own opacity.
    Merge { inputs: Vec<EffectInput>, alphas: Vec<f32> },
}

impl Shape {
    /// The brushed fill whose coverage clips the children, or `None` when they clip to
    /// [`Shape::clip`].
    pub fn coverage_clip(&self) -> Option<&Fill> {
        self.fill.as_ref().filter(|f| !f.marks.is_empty())
    }

    /// Local units the brush marks may paint past the fill's geometry or the ribbon's edge. 0
    /// without marks.
    pub fn mark_reach(&self) -> f32 {
        let fill = self.fill.as_ref().map_or(0.0, Fill::reach);
        let stroke = self.stroke.as_ref().and_then(|s| s.ribbon.as_ref()).map_or(0.0, Ribbon::reach);
        fill.max(stroke)
    }

    /// Whether a brush stage on the fill or the stroke reads `m.below`. Such a shape's paint
    /// is computed over the pixels already drawn under it, so it cannot be drawn offscreen.
    pub fn reads_below(&self) -> bool {
        self.fill.as_ref().is_some_and(Fill::reads_below)
            || self.stroke.as_ref().and_then(|s| s.ribbon.as_ref()).is_some_and(Ribbon::reads_below)
    }
}

/// The file, line and column a node's `origin` (`file:line:col`) names.
pub fn parse_origin(origin: &str) -> Option<(&str, u32, u32)> {
    let mut parts = origin.rsplitn(3, ':');
    let col = parts.next()?.parse().ok()?;
    let line = parts.next()?.parse().ok()?;
    Some((parts.next()?, line, col))
}

impl Node {
    /// The node's own mask, then the mask of each `fx outline`, in effect order. Every mask
    /// draws in this node's local coordinates, and the nodes of each are walked with its own.
    pub fn masks(&self) -> impl Iterator<Item = &Mask> + '_ {
        let outlines = self.effects.iter().filter_map(|e| match &e.op {
            EffectOp::Outline { mask, .. } => mask.as_deref(),
            _ => None,
        });
        self.mask.as_deref().into_iter().chain(outlines)
    }

    /// Every node directly under this one, in draw order: a shape's stamps, then its children,
    /// or a group's children. Every walker over the tree reads this. Only a renderer tells a
    /// shape's stamps and children apart, through [`Shape`]'s own fields, because only it clips.
    pub fn descendants(&self) -> impl Iterator<Item = &Node> + '_ {
        let (first, then): (&[Node], &[Node]) = match &self.kind {
            NodeKind::Group { children, .. } => (children, &[]),
            NodeKind::Shape(s) => (&s.stamps, &s.children),
        };
        first.iter().chain(then)
    }
}

/// The least alpha an 8-bit pixel stores as nonzero: output rounds to the nearest of 255 steps.
const INK: f64 = 0.5 / 255.0;

/// How far past an edge a Gaussian blur of `sigma`, drawn at `alpha`, still inks a pixel.
/// Outside a box, the worst case is a half-plane edge, whose blurred alpha at distance `d` is
/// `alpha * Q(d / sigma)`, with `Q` the Gaussian upper tail; the reach is where that falls
/// below [`INK`]. `None` when the blurred copy inks nothing.
pub fn blur_reach(sigma: f32, alpha: f32) -> Option<f32> {
    let alpha = f64::from(alpha.clamp(0.0, 1.0));
    if alpha <= INK {
        return None;
    }
    if sigma <= 0.0 || !sigma.is_finite() {
        return Some(0.0);
    }
    Some((f64::from(sigma) * upper_tail_inverse(INK / alpha)) as f32)
}

/// Smallest `x >= 0` with `Q(x) <= p`, by bisection, so the result is the same on every run.
fn upper_tail_inverse(p: f64) -> f64 {
    let (mut lo, mut hi) = (0.0_f64, 40.0_f64);
    if upper_tail(lo) <= p {
        return 0.0;
    }
    for _ in 0..80 {
        let mid = 0.5 * (lo + hi);
        if upper_tail(mid) > p {
            lo = mid
        } else {
            hi = mid
        }
    }
    hi
}

/// Gaussian upper tail `Q(x) = erfc(x / sqrt 2) / 2` for `x >= 0`, through the Chebyshev
/// fit of `erfc` from Numerical Recipes (fractional error below 1.2e-7).
fn upper_tail(x: f64) -> f64 {
    let z = x / std::f64::consts::SQRT_2;
    let t = 1.0 / (1.0 + 0.5 * z);
    let poly = -1.265_512_23
        + t * (1.000_023_68
            + t * (0.374_091_96
                + t * (0.096_784_18
                    + t * (-0.186_288_06
                        + t * (0.278_868_07
                            + t * (-1.135_203_98
                                + t * (1.488_515_87 + t * (-0.822_152_23 + t * 0.170_872_77))))))));
    0.5 * t * (-z * z + poly).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blur_reach_matches_the_gaussian_quantile() {
        let near = |a: f32, b: f64| (f64::from(a) - b).abs() < 1e-4;
        assert!(near(blur_reach(1.0, 1.0).unwrap(), 2.884_402_7));
        assert!(near(blur_reach(10.0, 0.4).unwrap() / 10.0, 2.582_669_4));
        assert_eq!(blur_reach(0.0, 1.0), Some(0.0));
        assert_eq!(blur_reach(8.0, 0.4 / 255.0), None);
    }
}
