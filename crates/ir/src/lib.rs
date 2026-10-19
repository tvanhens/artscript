//! Scene IR: a fully resolved tree that every backend reads.
//!
//! All numbers are absolute canvas units. Geometry is final (booleans, splines, tapered strokes
//! and jitter already applied). Gradients carry dense sRGB stops already interpolated in OKLCH,
//! so backends interpolate linearly between neighbouring stops and still match each other.

use std::collections::HashMap;
use std::sync::Arc;

pub use artscript_brush::UniformValue;
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
    pub program: Arc<artscript_brush::Program>,
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
    /// The filled outline of a tapered or width-list ribbon, the ink an unbrushed stroke of the
    /// same widths would fill. Outline effects, children and `inside` read it. `None` at
    /// constant width, where those readers use the geometry and its stroke at `width`, as they
    /// do for an unbrushed stroke.
    pub ink: Option<Path>,
}

impl Ribbon {
    pub fn reach(&self) -> f32 {
        self.marks.iter().map(|m| m.reach).fold(0.0, f32::max)
    }
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
    /// geometry's box is dropped on every side `sides` leaves out.
    Outline { width: f32, color: Color, sides: Sides },
    /// 3-octave Perlin inside the input's alpha, overlay-blended. `strength` is the brightness
    /// swing (.05 = 5%): the overlay opacity is `min(1, 2 * strength)`. `scale` is the finest
    /// feature in canvas units.
    Grain { strength: f32, scale: f32, seed: u32 },
    /// Composites `inputs` bottom to top, each with its own opacity.
    Merge { inputs: Vec<EffectInput>, alphas: Vec<f32> },
}

impl Shape {
    /// What the shape's children, `inside` and `copy` clip to: the geometry, as for any shape,
    /// except that a brushed tapered stroke contributes its ink outline in place of the spine,
    /// joined with the geometry when the shape also has a fill.
    pub fn clip(&self) -> std::borrow::Cow<'_, Path> {
        let ink = self.stroke.as_ref().and_then(|s| s.ribbon.as_ref()).and_then(|r| r.ink.as_ref());
        match ink {
            None => std::borrow::Cow::Borrowed(&self.geometry),
            Some(ink) if self.fill.is_none() => std::borrow::Cow::Borrowed(ink),
            Some(ink) => std::borrow::Cow::Owned(
                skia_safe::op(&self.geometry, ink, skia_safe::PathOp::Union).unwrap_or_else(|| ink.clone()),
            ),
        }
    }

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
}

impl Node {
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

    /// Visual bounds in this node's parent coordinates: [`Node::local_bounds`] through the
    /// transform. Effects are ignored; see [`Node::outer_bounds`] for the extent they draw.
    pub fn bounds(&self) -> Option<Rect> {
        self.local_bounds().map(|local| self.transform.map_rect(local).0)
    }

    /// Visual bounds in this node's own coordinates: geometry plus half the stroke width and
    /// the stamps, union of children for groups, cut to [`Node::kept_bounds`]. Effects are
    /// ignored.
    pub fn local_bounds(&self) -> Option<Rect> {
        let local = match &self.kind {
            NodeKind::Shape(s) => shape_bounds(s),
            NodeKind::Group { children, .. } => union_bounds(children),
        }?;
        self.cut_to_kept(local)
    }

    /// [`Node::local_bounds`] with each stroke measured by the outline it fills, so miter joins
    /// and square caps count.
    fn local_ink(&self) -> Option<Rect> {
        let local = match &self.kind {
            NodeKind::Shape(s) => join(shape_ink(s), union_ink(&s.stamps)),
            NodeKind::Group { children, .. } => union_ink(children),
        }?;
        self.cut_to_kept(local)
    }

    fn cut_to_kept(&self, mut local: Rect) -> Option<Rect> {
        for kept in self.kept_bounds() {
            if !local.intersect(kept) {
                return None;
            }
        }
        Some(local)
    }

    /// Drawn extent in this node's parent coordinates: [`Node::bounds`] grown by what the
    /// shape's `blur:` and the node's effects paint past it. Effects run after the node's own
    /// clip and mask, so those cut the content and not the effects. `find` returns a node by
    /// path with its parent's world transform, for effects that read another node (`of:@x`).
    pub fn outer_bounds<'n>(
        &self,
        parent_world: &Matrix,
        find: &dyn Fn(&str) -> Option<(&'n Node, Matrix)>,
    ) -> Option<Rect> {
        self.outer(parent_world, find, &mut Vec::new())
    }

    /// `visiting` holds the nodes being measured, as render's draw does: a node that reads
    /// itself through an effect input draws nothing there.
    fn outer<'n>(
        &self,
        parent_world: &Matrix,
        find: &dyn Fn(&str) -> Option<(&'n Node, Matrix)>,
        visiting: &mut Vec<String>,
    ) -> Option<Rect> {
        if visiting.contains(&self.path) {
            return None;
        }
        visiting.push(self.path.clone());
        let world = *parent_world * self.transform;
        let local = self.outer_local(&world, find, visiting);
        visiting.pop();
        Some(self.transform.map_rect(local?).0)
    }

    /// Folds the effect chain the way render's `effects::chain` does. A positive outline ends
    /// a chain segment: after it, `self` is the outlined layer and earlier `as:` names are gone.
    fn outer_local<'n>(
        &self,
        world: &Matrix,
        find: &dyn Fn(&str) -> Option<(&'n Node, Matrix)>,
        visiting: &mut Vec<String>,
    ) -> Option<Rect> {
        let outer_of = |nodes: &[Node], visiting: &mut Vec<String>| {
            join_all(nodes.iter().map(|c| c.outer(world, find, visiting)).collect::<Vec<_>>())
        };
        let content = match &self.kind {
            NodeKind::Shape(s) => join(
                grow(shape_ink(s), blur_reach(s.blur, 1.0)),
                outer_of(&s.stamps, visiting),
            ),
            NodeKind::Group { children, .. } => outer_of(children, visiting),
        };
        let mut source = content.and_then(|r| self.cut_to_kept(r));
        let mut current = source;
        let mut named: HashMap<&str, Option<Rect>> = HashMap::new();
        let mut env = InputEnv {
            world,
            find,
            visiting,
        };
        for effect in &self.effects {
            if let EffectOp::Outline { width, sides, .. } = effect.op
                && width > 0.0
            {
                let traced = self.local_ink().map(|r| {
                    let reach = |on: bool| if on { width } else { 0.0 };
                    Rect::new(
                        r.left - reach(sides.left),
                        r.top - reach(sides.top),
                        r.right + reach(sides.right),
                        r.bottom + reach(sides.bottom),
                    )
                });
                current = join(current, traced);
                source = current;
                named.clear();
                continue;
            }
            let input = env.resolve(&effect.input, current, source, &named);
            current = match &effect.op {
                EffectOp::Blur { sigma } => grow(input, blur_reach(*sigma, 1.0)),
                EffectOp::Glow {
                    radius,
                    color,
                    strength,
                } => join(input, grow(input, blur_reach(*radius, color.a * strength))),
                EffectOp::Shadow {
                    dx,
                    dy,
                    blur,
                    color,
                } => join(
                    input,
                    grow(
                        input.map(|r| r.with_offset((*dx, *dy))),
                        blur_reach(*blur, color.a),
                    ),
                ),
                EffectOp::Merge { inputs, alphas } => join_all(
                    inputs
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| alphas.get(*i).copied().unwrap_or(1.0) > 0.0)
                        .map(|(_, i)| env.resolve(i, input, source, &named))
                        .collect::<Vec<_>>(),
                ),
                EffectOp::Inner { .. }
                | EffectOp::Rim { .. }
                | EffectOp::Adjust { .. }
                | EffectOp::Grain { .. }
                | EffectOp::Outline { .. } => input,
            };
            if let Some(name) = &effect.name {
                named.insert(name, current);
            }
        }
        current
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
    join_all(nodes.iter().map(Node::bounds).collect::<Vec<_>>())
}

/// Each node's [`Node::local_ink`] through its transform, joined.
fn union_ink(nodes: &[Node]) -> Option<Rect> {
    join_all(
        nodes
            .iter()
            .map(|c| Some(c.transform.map_rect(c.local_ink()?).0))
            .collect::<Vec<_>>(),
    )
}

/// Geometry plus half the stroke width, plus what the brush marks may paint past it, joined
/// with the stamps' bounds.
fn shape_bounds(s: &Shape) -> Option<Rect> {
    let mut r = s.geometry.compute_tight_bounds();
    if let Some(st) = &s.stroke {
        r.outset((st.width / 2.0, st.width / 2.0));
    }
    let reach = s.mark_reach();
    if reach > 0.0 {
        r.outset((reach, reach));
    }
    let own = if r.is_empty() && s.geometry.is_empty() { None } else { Some(r) };
    // Skia's `join` drops an empty rect, and an unstroked line is one, so cover both by hand.
    match (own, union_bounds(&s.stamps)) {
        (Some(a), Some(b)) => Some(Rect::new(a.left.min(b.left), a.top.min(b.top), a.right.max(b.right), a.bottom.max(b.bottom))),
        (a, b) => a.or(b),
    }
}

/// Geometry joined with the outline its stroke fills, at Skia's default miter limit as render
/// draws it. A dash only removes ink, so it is ignored.
fn shape_ink(s: &Shape) -> Option<Rect> {
    let plain = shape_bounds(s)?;
    let Some(st) = s.stroke.as_ref().filter(|st| st.width > 0.0) else {
        return Some(plain);
    };
    let mut paint = skia_safe::Paint::default();
    paint.set_style(skia_safe::paint::Style::Stroke);
    paint.set_stroke_width(st.width);
    paint.set_stroke_cap(match st.cap {
        Cap::Round => skia_safe::paint::Cap::Round,
        Cap::Butt => skia_safe::paint::Cap::Butt,
        Cap::Square => skia_safe::paint::Cap::Square,
    });
    paint.set_stroke_join(match st.join {
        Join::Round => skia_safe::paint::Join::Round,
        Join::Miter => skia_safe::paint::Join::Miter,
        Join::Bevel => skia_safe::paint::Join::Bevel,
    });
    let mut filled = skia_safe::PathBuilder::new();
    skia_safe::path_utils::fill_path_with_paint(
        &s.geometry,
        &paint,
        &mut filled,
        None::<&Rect>,
        None::<Matrix>,
    );
    join(Some(plain), Some(filled.snapshot().compute_tight_bounds()).filter(|r| !r.is_empty()))
}

struct InputEnv<'a, 'f, 'n> {
    world: &'a Matrix,
    find: &'f dyn Fn(&str) -> Option<(&'n Node, Matrix)>,
    visiting: &'a mut Vec<String>,
}

impl InputEnv<'_, '_, '_> {
    /// `None` is an input that draws nothing, as an unknown `as:` name does in render.
    fn resolve(
        &mut self,
        input: &EffectInput,
        current: Option<Rect>,
        source: Option<Rect>,
        named: &HashMap<&str, Option<Rect>>,
    ) -> Option<Rect> {
        match input {
            EffectInput::Previous => current,
            EffectInput::Source => source,
            EffectInput::Named(name) => named.get(name.as_str()).copied().flatten(),
            EffectInput::Node(path) => {
                let (node, parent_world) = (self.find)(path)?;
                let canvas = parent_world
                    .map_rect(node.outer(&parent_world, self.find, self.visiting)?)
                    .0;
                Some(self.world.invert()?.map_rect(canvas).0)
            }
        }
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

fn grow(r: Option<Rect>, reach: Option<f32>) -> Option<Rect> {
    Some(r?.with_outset((reach?, reach?)))
}

fn join(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    join_all([a, b])
}

fn join_all(rects: impl IntoIterator<Item = Option<Rect>>) -> Option<Rect> {
    rects.into_iter().flatten().reduce(|mut a, b| {
        a.join(b);
        a
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape_node(path: &str, geometry: Path, stamps: Vec<Node>) -> Node {
        Node {
            path: path.into(),
            origin: String::new(),
            transform: Matrix::new_identity(),
            opacity: 1.0,
            blend: Blend::Normal,
            clip: None,
            mask: None,
            effects: Vec::new(),
            kind: NodeKind::Shape(Box::new(Shape {
                geometry,
                fill: Some(Fill::plain(Paint::Solid(Color::TRANSPARENT))),
                stroke: None,
                blur: 0.0,
                stamps,
                children: Vec::new(),
            })),
        }
    }

    #[test]
    fn a_shape_s_bounds_and_outer_bounds_grow_through_its_stamps() {
        let mut leaf = shape_node("vine.leaves.0", Path::rect(Rect::from_xywh(-4.0, -4.0, 8.0, 8.0), None), Vec::new());
        leaf.transform = Matrix::translate((100.0, 0.0));
        leaf.effects.push(Effect {
            op: EffectOp::Glow { radius: 2.0, color: Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }, strength: 1.0 },
            input: EffectInput::Previous,
            name: None,
        });
        let host = shape_node("vine", Path::rect(Rect::from_xywh(0.0, 0.0, 50.0, 10.0), None), vec![leaf]);
        let b = host.bounds().unwrap();
        assert_eq!((b.left, b.top, b.right, b.bottom), (0.0, -4.0, 104.0, 10.0));
        let outer = host.outer_bounds(&Matrix::new_identity(), &|_| None).unwrap();
        let reach = blur_reach(2.0, 1.0).unwrap();
        assert!((outer.right - (104.0 + reach)).abs() < 1e-4, "{outer:?}");
        assert!((outer.top - (-4.0 - reach)).abs() < 1e-4, "{outer:?}");
        let paths: Vec<&str> = host.descendants().map(|n| n.path.as_str()).collect();
        assert_eq!(paths, ["vine.leaves.0"]);
    }

    #[test]
    fn blur_reach_matches_the_gaussian_quantile() {
        let near = |a: f32, b: f64| (f64::from(a) - b).abs() < 1e-4;
        assert!(near(blur_reach(1.0, 1.0).unwrap(), 2.884_402_7));
        assert!(near(blur_reach(10.0, 0.4).unwrap() / 10.0, 2.582_669_4));
        assert_eq!(blur_reach(0.0, 1.0), Some(0.0));
        assert_eq!(blur_reach(8.0, 0.4 / 255.0), None);
    }
}
