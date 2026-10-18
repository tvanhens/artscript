//! Scene IR: a fully resolved tree that every backend reads.
//!
//! All numbers are absolute canvas units. Geometry is final (booleans, splines, tapered strokes
//! and jitter already applied). Gradients carry dense sRGB stops already interpolated in OKLCH,
//! so backends interpolate linearly between neighbouring stops and still match each other.

pub use skia_safe::{Matrix, Path, Point, Rect};

mod retained;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    /// Pure SVG; any raster-only feature is an error.
    Vector,
    /// SVG, raster-only subtrees baked to embedded PNG tiles.
    Svg,
    /// Skia renders directly.
    Raster,
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
    /// Stable dotted path, e.g. `hero.body.arm_r`. Unnamed nodes get `<verb><index>`.
    pub path: String,
    /// Source location `file:line` for diagnostics and `explain`.
    pub origin: String,
    /// Local to parent.
    pub transform: Matrix,
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
    pub fill: Option<Paint>,
    pub stroke: Option<Stroke>,
    /// Gaussian sigma that softens the shape's own edge (Skia mask-filter blur). 0 = crisp.
    pub blur: f32,
    /// Drawn after the shape, clipped to `geometry` whatever the shape's fill or alpha (the
    /// "clip by nesting" rule). The shape and its children form one isolated group, so a
    /// child's blend mode never reaches content below the shape.
    pub children: Vec<Node>,
}

#[derive(Clone, Debug)]
pub struct Stroke {
    pub paint: Paint,
    pub width: f32,
    pub cap: Cap,
    pub join: Join,
    pub dash: Vec<f32>,
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
    /// `scale` is the feature size in canvas units.
    Noise { kind: NoiseKind, scale: f32, seed: u32 },
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
    /// Soft halo of `color` behind the input, `radius` sigma.
    Glow { radius: f32, color: Color },
    /// Thin light band just inside the edge on the side facing `dir` (screen space, y down),
    /// brightest at the edge and fading inward over `width`.
    Rim { width: f32, dir: Point, color: Color },
    /// OKLCH shift of every pixel: hue degrees, chroma fraction (+.2 = 20% more), OKLab lightness.
    Adjust { hue: f32, sat: f32, light: f32 },
    /// Line of `width` around the node's shape geometry, drawn behind the content.
    /// Earlier effects do not change the traced path.
    Outline { width: f32, color: Color },
    /// 3-octave Perlin inside the input's alpha, overlay-blended. `strength` is the brightness
    /// swing (.05 = 5%): the overlay opacity is `min(1, 2 * strength)`. `scale` is the finest
    /// feature in canvas units.
    Grain { strength: f32, scale: f32, seed: u32 },
    /// Composites `inputs` bottom to top, each with its own opacity.
    Merge { inputs: Vec<EffectInput>, alphas: Vec<f32> },
}

impl Node {
    pub fn children(&self) -> &[Node] {
        match &self.kind {
            NodeKind::Group { children, .. } => children,
            NodeKind::Shape(s) => &s.children,
        }
    }

    /// Visual bounds in this node's parent coordinates: [`Node::local_bounds`] through the transform.
    pub fn bounds(&self) -> Option<Rect> {
        self.local_bounds().map(|local| self.transform.map_rect(local).0)
    }

    /// Visual bounds in this node's own coordinates: geometry plus half the stroke width,
    /// union of children for groups, cut to [`Node::kept_bounds`]. Effects are ignored.
    pub fn local_bounds(&self) -> Option<Rect> {
        let mut local = match &self.kind {
            NodeKind::Shape(s) => {
                let mut r = s.geometry.compute_tight_bounds();
                if let Some(st) = &s.stroke {
                    r.outset((st.width / 2.0, st.width / 2.0));
                }
                if r.is_empty() && s.geometry.is_empty() { None } else { Some(r) }
            }
            NodeKind::Group { children, .. } => union_bounds(children),
        }?;
        for kept in self.kept_bounds() {
            if !local.intersect(kept) {
                return None;
            }
        }
        Some(local)
    }

    /// Local bounds this node's `inside` clip and node mask keep, grown by three feather sigmas.
    /// `outside`, inverted masks, and field masks keep everything.
    pub fn kept_bounds(&self) -> impl Iterator<Item = Rect> + '_ {
        let clip = self.clip.as_ref().filter(|c| !c.invert).map(|c| {
            c.path
                .compute_tight_bounds()
                .with_outset((3.0 * c.feather, 3.0 * c.feather))
        });
        let mask = self
            .mask
            .as_ref()
            .filter(|m| !m.invert)
            .and_then(|m| match &m.source {
                MaskSource::Nodes(nodes) => {
                    union_bounds(nodes).map(|r| r.with_outset((3.0 * m.feather, 3.0 * m.feather)))
                }
                MaskSource::Field { .. } => None,
            });
        clip.into_iter().chain(mask)
    }
}

pub fn union_bounds(nodes: &[Node]) -> Option<Rect> {
    nodes.iter().filter_map(Node::bounds).reduce(|mut a, b| {
        a.join(b);
        a
    })
}
