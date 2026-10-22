//! Bounds and placement of IR nodes, measured with Skia the way render draws them.

use std::collections::HashMap;

use ramose_ir::{Cap, EffectInput, EffectOp, Join, MaskSource, Node, NodeKind, Shape, blur_reach, ribbon_reach};
use skia_safe::{Matrix, Path, Point, Rect};

use crate::{ToSk, WarpGeom};

/// Local units a bent outline may stray from its true image when a box is measured through it.
const PLACE_TOL: f32 = 0.05;

/// Finds a node by path with its parent's world transform, for effects that read another node
/// (`of:@x`).
pub type Find<'f, 'n> = &'f dyn Fn(&str) -> Option<(&'n Node, Matrix)>;

pub trait ShapeGeom {
    /// This shape's geometry, stroke, and brush reach in local units, before stamps or children.
    fn own_bounds(&self) -> Option<Rect>;
    /// What the shape's children, `inside` and `copy` clip to: the geometry, as for any shape,
    /// except that a tapered stroke contributes its ink outline in place of the spine, joined
    /// with the geometry when the shape also has a fill.
    fn clip(&self) -> Path;
}

impl ShapeGeom for Shape {
    fn own_bounds(&self) -> Option<Rect> {
        let geometry = self.geometry.to_sk();
        let mut r = geometry.compute_tight_bounds();
        match self.stroke.as_ref().map(|st| (st, st.ink.as_ref().filter(|_| st.ribbon.is_none()))) {
            Some((_, Some(ink))) if self.fill.is_none() => r = ink.to_sk().compute_tight_bounds(),
            Some((_, Some(ink))) => r.join(ink.to_sk().compute_tight_bounds()),
            Some((st, None)) => r.outset((st.width / 2.0, st.width / 2.0)),
            None => {}
        }
        let reach = self.mark_reach();
        if reach > 0.0 {
            r.outset((reach, reach));
        }
        if r.is_empty() && geometry.is_empty() { None } else { Some(r) }
    }

    fn clip(&self) -> Path {
        let geometry = self.geometry.to_sk();
        match self.stroke.as_ref().and_then(|s| s.ink.as_ref()) {
            None => geometry,
            Some(ink) if self.fill.is_none() => ink.to_sk(),
            Some(ink) => {
                let ink = ink.to_sk();
                skia_safe::op(&geometry, &ink, skia_safe::PathOp::Union).unwrap_or(ink)
            }
        }
    }
}

pub trait NodeGeom {
    /// Visual bounds in this node's parent coordinates: [`NodeGeom::local_bounds`] placed
    /// through the warp and the transform. Effects are ignored; see [`NodeGeom::outer_bounds`]
    /// for the extent they draw.
    fn bounds(&self) -> Option<Rect>;
    /// The box stage 0 of a warp maps: the node's ink box in its own frame before any clip or
    /// mask cuts it (the shape's own bounds, or the box of a group's children). `None` when the
    /// node has no content. Eval calls it once, when the node's body closes, to resolve the warp.
    fn reference_box(&self) -> Option<Rect>;
    /// Content frame to parent: through the warp, then `transform`. The one owner of that
    /// composition.
    fn place_rect(&self, local: Rect) -> Rect;
    fn place_path(&self, local: &Path, tol: f32) -> Path;
    /// `None` for a point the warp hides.
    fn place_point(&self, local: Point) -> Option<Point>;
    /// Parent units per content unit, at most: the transform's scale times the warp's largest
    /// magnification over the reference box.
    fn place_scale(&self) -> f32;
    /// The pre-warp drawn extent in the content frame: the ink grown by the shape's `blur:`,
    /// joined with what the stamps draw, cut to [`NodeGeom::kept_bounds`]. Render sizes a warped
    /// node's flat offscreen with it; [`NodeGeom::outer_bounds`] warps it and folds the effects.
    fn content_reach<'n>(&self, world: &Matrix, find: Find<'_, 'n>) -> Option<Rect>;
    /// Visual bounds in this node's own content frame, before the warp: geometry plus half the
    /// stroke width and the stamps, union of children for groups, cut to
    /// [`NodeGeom::kept_bounds`]. Effects are ignored.
    fn local_bounds(&self) -> Option<Rect>;
    /// Drawn extent in this node's parent coordinates: [`NodeGeom::bounds`] grown by what the
    /// shape's `blur:` and the node's effects paint past it. Effects run after the node's own
    /// clip and mask, so those cut the content and not the effects.
    fn outer_bounds<'n>(&self, parent_world: &Matrix, find: Find<'_, 'n>) -> Option<Rect>;
    /// Local bounds this node's `inside` clip and node mask keep, grown by three feather sigmas.
    /// `outside`, inverted masks, and field masks keep everything.
    fn kept_bounds(&self) -> impl Iterator<Item = Rect> + '_;
}

impl NodeGeom for Node {
    fn bounds(&self) -> Option<Rect> {
        self.local_bounds().map(|local| self.place_rect(local))
    }

    fn reference_box(&self) -> Option<Rect> {
        match &self.kind {
            NodeKind::Shape(s) => shape_bounds(s),
            NodeKind::Group { children, .. } => union_bounds(children),
        }
    }

    fn place_rect(&self, local: Rect) -> Rect {
        if self.warp.is_none() {
            return self.transform.to_sk().map_rect(local).0;
        }
        // The warped outline is a quad or a bent band, so its box is taken after the
        // transform, where boxing the local result first would grow it under a rotation.
        self.place_path(&Path::rect(local, None), PLACE_TOL).compute_tight_bounds()
    }

    fn place_path(&self, local: &Path, tol: f32) -> Path {
        self.warp.map_path(local, tol).with_transform(&self.transform.to_sk())
    }

    fn place_point(&self, local: Point) -> Option<Point> {
        self.warp.map_point(local).map(|p| self.transform.to_sk().map_point(p))
    }

    fn place_scale(&self) -> f32 {
        let stretch = self.warp.reference().map_or(1.0, |r| self.warp.max_stretch(r.to_sk()));
        transform_scale(&self.transform.to_sk()) * stretch
    }

    fn content_reach<'n>(&self, world: &Matrix, find: Find<'_, 'n>) -> Option<Rect> {
        content_reach_in(self, world, find, &mut vec![self.path.clone()])
    }

    fn local_bounds(&self) -> Option<Rect> {
        let local = match &self.kind {
            NodeKind::Shape(s) => shape_bounds(s),
            NodeKind::Group { children, .. } => union_bounds(children),
        }?;
        cut_to_kept(self, local)
    }

    fn outer_bounds<'n>(&self, parent_world: &Matrix, find: Find<'_, 'n>) -> Option<Rect> {
        outer(self, parent_world, find, &mut Vec::new())
    }

    fn kept_bounds(&self) -> impl Iterator<Item = Rect> + '_ {
        let clip = self.clip.as_ref().filter(|c| !c.invert).map(|c| {
            c.path
                .to_sk()
                .compute_tight_bounds()
                .with_outset((3.0 * c.feather, 3.0 * c.feather))
        });
        let mask = self.mask.as_ref().filter(|m| !m.invert).and_then(|m| match &m.source {
            MaskSource::Nodes(nodes) => union_bounds(nodes).map(|r| r.with_outset((3.0 * m.feather, 3.0 * m.feather))),
            MaskSource::Field { .. } => None,
        });
        clip.into_iter().chain(mask)
    }
}

fn content_reach_in<'n>(node: &Node, world: &Matrix, find: Find<'_, 'n>, visiting: &mut Vec<String>) -> Option<Rect> {
    let outer_of = |nodes: &[Node], visiting: &mut Vec<String>| {
        join_all(nodes.iter().map(|c| outer(c, world, find, visiting)).collect::<Vec<_>>())
    };
    let content = match &node.kind {
        NodeKind::Shape(s) => join(grow(shape_ink(s), blur_reach(s.blur, 1.0)), outer_of(&s.stamps, visiting)),
        NodeKind::Group { children, .. } => outer_of(children, visiting),
    };
    content.and_then(|r| cut_to_kept(node, r))
}

/// [`NodeGeom::local_bounds`] with each stroke measured by the outline it fills, so miter joins
/// and square caps count.
fn local_ink(node: &Node) -> Option<Rect> {
    let local = match &node.kind {
        NodeKind::Shape(s) => join(shape_ink(s), union_ink(&s.stamps)),
        NodeKind::Group { children, .. } => union_ink(children),
    }?;
    cut_to_kept(node, local)
}

fn cut_to_kept(node: &Node, mut local: Rect) -> Option<Rect> {
    for kept in node.kept_bounds() {
        if !local.intersect(kept) {
            return None;
        }
    }
    Some(local)
}

/// `visiting` holds the nodes being measured, as render's draw does: a node that reads
/// itself through an effect input draws nothing there.
fn outer<'n>(node: &Node, parent_world: &Matrix, find: Find<'_, 'n>, visiting: &mut Vec<String>) -> Option<Rect> {
    if visiting.contains(&node.path) {
        return None;
    }
    visiting.push(node.path.clone());
    let transform = node.transform.to_sk();
    let world = *parent_world * transform;
    let local = outer_local(node, &world, find, visiting);
    visiting.pop();
    Some(transform.map_rect(local?).0)
}

/// Folds the effect chain the way render's `effects::chain` does, over the warped content,
/// in the frame `transform` places. A positive outline ends a chain segment: after it,
/// `self` is the outlined layer and earlier `as:` names are gone.
fn outer_local<'n>(node: &Node, world: &Matrix, find: Find<'_, 'n>, visiting: &mut Vec<String>) -> Option<Rect> {
    let mut source = content_reach_in(node, world, find, visiting).map(|r| node.warp.map_rect(r));
    let mut current = source;
    let mut named: HashMap<&str, Option<Rect>> = HashMap::new();
    let mut env = InputEnv { world, find, visiting };
    for effect in &node.effects {
        if let EffectOp::Outline { width, sides, brush, .. } = &effect.op
            && *width > 0.0
        {
            let (width, sides) = (*width + ribbon_reach(brush), *sides);
            let traced = local_ink(node).map(|r| {
                let r = node.warp.map_rect(r);
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
            EffectOp::Glow { radius, color, strength } => join(input, grow(input, blur_reach(*radius, color.a * strength))),
            EffectOp::Shadow { dx, dy, blur, color } => {
                join(input, grow(input.map(|r| r.with_offset((*dx, *dy))), blur_reach(*blur, color.a)))
            }
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

pub fn union_bounds(nodes: &[Node]) -> Option<Rect> {
    join_all(nodes.iter().map(Node::bounds).collect::<Vec<_>>())
}

/// Each node's [`local_ink`] placed in its parent, joined.
fn union_ink(nodes: &[Node]) -> Option<Rect> {
    join_all(nodes.iter().map(|c| Some(c.place_rect(local_ink(c)?))).collect::<Vec<_>>())
}

/// The most parent units one local unit spans under `m`, at least 1.
pub fn transform_scale(m: &Matrix) -> f32 {
    let sx = (m.scale_x() * m.scale_x() + m.skew_y() * m.skew_y()).sqrt();
    let sy = (m.scale_y() * m.scale_y() + m.skew_x() * m.skew_x()).sqrt();
    sx.max(sy).max(1.0)
}

/// Geometry plus half the stroke width, plus what the brush marks may paint past it, joined
/// with the stamps' bounds. An unbrushed tapered stroke contributes its ink instead, in place of
/// the geometry when the shape has no fill. A brushed stroke keeps the half width, since its
/// marks paint from the ribbon's edge, not from the ink's flattened outline.
fn shape_bounds(s: &Shape) -> Option<Rect> {
    let own = s.own_bounds();
    // Skia's `join` drops an empty rect, and an unstroked line is one, so cover both by hand.
    match (own, union_bounds(&s.stamps)) {
        (Some(a), Some(b)) => {
            Some(Rect::new(a.left.min(b.left), a.top.min(b.top), a.right.max(b.right), a.bottom.max(b.bottom)))
        }
        (a, b) => a.or(b),
    }
}

/// Geometry joined with the outline its stroke fills, at Skia's default miter limit as render
/// draws it. A dash only removes ink, so it is ignored.
fn shape_ink(s: &Shape) -> Option<Rect> {
    let plain = shape_bounds(s)?;
    let Some(st) = s.stroke.as_ref().filter(|st| st.width > 0.0 && st.ink.is_none()) else {
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
    skia_safe::path_utils::fill_path_with_paint(&s.geometry.to_sk(), &paint, &mut filled, None::<&Rect>, None::<Matrix>);
    join(Some(plain), Some(filled.snapshot().compute_tight_bounds()).filter(|r| !r.is_empty()))
}

struct InputEnv<'a, 'f, 'n> {
    world: &'a Matrix,
    find: Find<'f, 'n>,
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
                let artboard = parent_world.map_rect(outer(node, &parent_world, self.find, self.visiting)?).0;
                Some(self.world.invert()?.map_rect(artboard).0)
            }
        }
    }
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
    use ramose_ir::{Blend, Color, Corners, Effect, Fill, Paint, Warp, WarpStage};

    use super::*;
    use crate::ToIr;

    fn shape_node(path: &str, geometry: Path, stamps: Vec<Node>) -> Node {
        Node {
            path: path.into(),
            origin: String::new(),
            transform: ramose_ir::Matrix::IDENTITY,
            warp: Warp::default(),
            opacity: 1.0,
            blend: Blend::Normal,
            clip: None,
            mask: None,
            effects: Vec::new(),
            kind: NodeKind::Shape(Box::new(Shape {
                geometry: geometry.to_ir(),
                fill: Some(Fill::plain(Paint::Solid(Color::TRANSPARENT))),
                stroke: None,
                blur: 0.0,
                stamps,
                children: Vec::new(),
            })),
        }
    }

    fn glow() -> Effect {
        Effect {
            op: EffectOp::Glow { radius: 2.0, color: Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }, strength: 1.0 },
            input: EffectInput::Previous,
            name: None,
        }
    }

    #[test]
    fn a_warped_node_measures_the_warped_box_and_its_effects_follow_it() {
        let quad = [(412.0, 180.0), (530.0, 196.0), (526.0, 330.0), (408.0, 322.0)].map(|(x, y)| ramose_ir::Point::new(x, y));
        let mut side = shape_node("side", Path::rect(Rect::from_xywh(0.0, 0.0, 40.0, 200.0), None), Vec::new());
        side.transform = Matrix::translate((100.0, 0.0)).to_ir();
        side.warp = Warp::resolve(&[WarpStage::Corners(Corners::try_new(quad).unwrap())], side.reference_box().unwrap())
            .unwrap();
        side.effects.push(glow());
        let near = |r: Rect, want: Rect| {
            (r.left - want.left).abs() < 1e-2
                && (r.top - want.top).abs() < 1e-2
                && (r.right - want.right).abs() < 1e-2
                && (r.bottom - want.bottom).abs() < 1e-2
        };
        let quad_box = Rect::new(508.0, 180.0, 630.0, 330.0);
        assert!(near(side.bounds().unwrap(), quad_box), "{:?}", side.bounds());
        assert_eq!(side.local_bounds(), Some(Rect::from_xywh(0.0, 0.0, 40.0, 200.0)));
        let reach = blur_reach(2.0, 1.0).unwrap();
        let outer = side.outer_bounds(&Matrix::new_identity(), &|_| None).unwrap();
        assert!(near(outer, quad_box.with_outset((reach, reach))), "{outer:?}");
        let flat = side.content_reach(&Matrix::new_identity(), &|_| None).unwrap();
        assert_eq!(flat, Rect::from_xywh(0.0, 0.0, 40.0, 200.0));
        assert!(side.place_scale() > 1.0);
        let group = Node {
            kind: NodeKind::Group { isolate: false, children: vec![side] },
            ..shape_node("book", Path::new(), Vec::new())
        };
        assert!(near(group.bounds().unwrap(), quad_box), "{:?}", group.bounds());
        assert!(near(group.reference_box().unwrap(), quad_box));
    }

    #[test]
    fn a_shape_s_bounds_and_outer_bounds_grow_through_its_stamps() {
        let mut leaf = shape_node("vine.leaves.0", Path::rect(Rect::from_xywh(-4.0, -4.0, 8.0, 8.0), None), Vec::new());
        leaf.transform = Matrix::translate((100.0, 0.0)).to_ir();
        leaf.effects.push(glow());
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
}
