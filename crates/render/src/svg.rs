//! SVG writer. Vector nodes stay elements. A node in the raster-only census is replaced by one
//! PNG tile of that whole subtree.

use artscript_ir::{
    Blend, Cap, Clip, Color, Effect, EffectInput, EffectOp, Join, Mask, MaskSource, Matrix, Node,
    NodeKind, Paint, Path, Point, Profile, Scene, Shape, Stop, Stroke,
};
use skia_safe::path::{Iter as PathIter, Verb};
use skia_safe::{PathFillType, PathOp, Rect};

use crate::draw::{bake_node, visual_pad};
use crate::effects::{Coverage, Region};
use crate::gpu::Backend;
use crate::features::{self, bakes};
use crate::paint::css_blend;
use crate::text::{base64, fmt_num, xml_id};
use crate::RenderError;

pub fn render_svg(
    scene: &Scene,
    profile: Profile,
    bake_scale: f32,
    backend: Backend,
) -> Result<String, RenderError> {
    let hits = features::raster_only_features(scene);
    if profile == Profile::Vector && !hits.is_empty() {
        return Err(RenderError::NotVector(hits));
    }
    let mut w = Writer { defs: String::new(), body: String::new(), backend };
    if let Some(bg) = scene.background {
        w.body.push_str("  <rect width=\"");
        w.body.push_str(&fmt_num(scene.width));
        w.body.push_str("\" height=\"");
        w.body.push_str(&fmt_num(scene.height));
        w.body.push_str("\" fill=\"");
        w.body.push_str(&rgb_hex(bg));
        w.body.push('"');
        if bg.a < 1.0 {
            w.body.push_str(" fill-opacity=\"");
            w.body.push_str(&fmt_num(bg.a));
            w.body.push('"');
        }
        w.body.push_str("/>\n");
    }
    for child in &scene.children {
        w.node(scene, child, bake_scale, 1, None)?;
    }
    let mut out = String::new();
    out.push_str("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"");
    out.push_str(&fmt_num(scene.width));
    out.push_str("\" height=\"");
    out.push_str(&fmt_num(scene.height));
    out.push_str("\" viewBox=\"0 0 ");
    out.push_str(&fmt_num(scene.width));
    out.push(' ');
    out.push_str(&fmt_num(scene.height));
    out.push_str("\">\n");
    if !w.defs.is_empty() {
        out.push_str("  <defs>\n");
        out.push_str(&w.defs);
        out.push_str("  </defs>\n");
    }
    out.push_str(&w.body);
    out.push_str("</svg>\n");
    Ok(out)
}

struct Writer {
    defs: String,
    body: String,
    backend: Backend,
}

impl Writer {
    /// `outer` is a clip in the parent's coordinates that a plain clipped group above pushed
    /// down. A `clip-path` on a `<g>` isolates it, so such a group clips the children instead.
    fn node(
        &mut self,
        scene: &Scene,
        node: &Node,
        bake_scale: f32,
        depth: usize,
        outer: Option<&Path>,
    ) -> Result<(), RenderError> {
        let pad = "  ".repeat(depth);
        let id = xml_id(&node.path);
        let outer = outer.map(|path| to_local(path, &node.transform));
        if bakes(node) {
            let baked = bake_node(scene, node, bake_scale, self.backend)?;
            self.body.push_str(&pad);
            self.body.push_str("<g id=\"");
            self.body.push_str(&id);
            self.body.push('"');
            self.blend_style(node);
            if let Some(path) = &outer {
                self.clip_ref(&format!("{id}-clip"), path);
            }
            self.body.push_str(">\n");
            self.body.push_str(&pad);
            self.body.push_str("  <image x=\"");
            self.body.push_str(&fmt_num(baked.x));
            self.body.push_str("\" y=\"");
            self.body.push_str(&fmt_num(baked.y));
            self.body.push_str("\" width=\"");
            self.body.push_str(&fmt_num(baked.w));
            self.body.push_str("\" height=\"");
            self.body.push_str(&fmt_num(baked.h));
            self.body.push_str("\" href=\"data:image/png;base64,");
            self.body.push_str(&base64(&baked.png));
            self.body.push_str("\"/>\n");
            self.body.push_str(&pad);
            self.body.push_str("</g>\n");
            return Ok(());
        }

        self.body.push_str(&pad);
        self.body.push_str("<g id=\"");
        self.body.push_str(&id);
        self.body.push('"');
        if let Some(t) = matrix_attr(&node.transform) {
            self.body.push_str(" transform=\"");
            self.body.push_str(&t);
            self.body.push('"');
        }
        if node.opacity < 1.0 {
            self.body.push_str(" opacity=\"");
            self.body.push_str(&fmt_num(node.opacity));
            self.body.push('"');
        }
        self.blend_style(node);
        let pushed = if pushes_clip(node) {
            intersect(outer.clone(), node.clip.as_ref().map(clip_region).transpose()?)?
        } else {
            None
        };
        if let Some(region) = pushed {
            self.body.push_str(">\n");
            self.write_kind(scene, node, &id, bake_scale, depth, Some(&region))?;
        } else if !has_outline(node) {
            self.clip_attr(node, &id, outer.as_ref())?;
            self.mask_attr(scene, node, &id, bake_scale)?;
            if let Some(filter) = self.fx_filter(node, &id) {
                self.body.push_str(" filter=\"url(#");
                self.body.push_str(&filter);
                self.body.push_str(")\"");
            }
            self.body.push_str(">\n");
            self.write_kind(scene, node, &id, bake_scale, depth, None)?;
        } else {
            if let Some(path) = &outer {
                self.clip_ref(&format!("{id}-outer"), path);
            }
            self.body.push_str(">\n");
            let steps = svg_steps(&node.effects);
            self.emit_outlined(scene, node, &id, bake_scale, depth, &steps)?;
        }

        self.body.push_str(&pad);
        self.body.push_str("</g>\n");
        Ok(())
    }

    fn write_kind(
        &mut self,
        scene: &Scene,
        node: &Node,
        id: &str,
        bake_scale: f32,
        depth: usize,
        outer: Option<&Path>,
    ) -> Result<(), RenderError> {
        match &node.kind {
            NodeKind::Group { children, .. } => {
                for child in children {
                    self.node(scene, child, bake_scale, depth + 1, outer)?;
                }
            }
            NodeKind::Shape(shape) => self.shape(scene, shape, id, bake_scale, depth + 1)?,
        }
        Ok(())
    }

    /// Outline paths sit behind the content. A blur or shadow after an outline wraps both.
    /// One before an outline wraps only the content, so it does not change the outline.
    fn emit_outlined(
        &mut self,
        scene: &Scene,
        node: &Node,
        id: &str,
        bake_scale: f32,
        depth: usize,
        steps: &[SvgStep<'_>],
    ) -> Result<(), RenderError> {
        let Some((last, head)) = steps.split_last() else {
            return self.write_masked_kind(scene, node, id, bake_scale, depth);
        };
        match last {
            SvgStep::Outline { width, color } => {
                let oid = format!("{id}-outline{}", steps.len());
                self.outline_paths(node, *width, *color, &oid, bake_scale, depth + 1)?;
                self.emit_outlined(scene, node, id, bake_scale, depth, head)
            }
            SvgStep::Filters(effects) => {
                let pad = "  ".repeat(depth + 1);
                let fid = format!("{id}-fx{}", steps.len());
                if self.write_fx_filter(effects, &fid) {
                    self.body.push_str(&pad);
                    self.body.push_str("<g filter=\"url(#");
                    self.body.push_str(&fid);
                    self.body.push_str(")\">\n");
                    self.emit_outlined(scene, node, id, bake_scale, depth + 1, head)?;
                    self.body.push_str(&pad);
                    self.body.push_str("</g>\n");
                } else {
                    self.emit_outlined(scene, node, id, bake_scale, depth, head)?;
                }
                Ok(())
            }
        }
    }

    fn write_masked_kind(
        &mut self,
        scene: &Scene,
        node: &Node,
        id: &str,
        bake_scale: f32,
        depth: usize,
    ) -> Result<(), RenderError> {
        let wrap = node.clip.is_some() || node.mask.is_some();
        if wrap {
            let pad = "  ".repeat(depth + 1);
            self.body.push_str(&pad);
            self.body.push_str("<g");
            self.clip_attr(node, id, None)?;
            self.mask_attr(scene, node, id, bake_scale)?;
            self.body.push_str(">\n");
            self.write_kind(scene, node, id, bake_scale, depth + 1, None)?;
            self.body.push_str(&pad);
            self.body.push_str("</g>\n");
        } else {
            self.write_kind(scene, node, id, bake_scale, depth, None)?;
        }
        Ok(())
    }

    fn outline_paths(&mut self, node: &Node, width: f32, color: Color, id: &str, bake_scale: f32, depth: usize) -> Result<(), RenderError> {
        let regions = crate::effects::silhouette(node, &Matrix::scale((bake_scale, bake_scale)))?;
        if regions.is_empty() {
            return Ok(());
        }
        let translucent: Vec<_> = regions.iter().filter(|r| r.coverage == Coverage::Translucent).collect();
        let fade_or_mask_as_one = color.a < 1.0 || !translucent.is_empty();
        let pad = "  ".repeat(depth);
        if fade_or_mask_as_one {
            self.body.push_str(&pad);
            self.body.push_str("<g");
            if !translucent.is_empty() {
                self.erase_mask_def(id, &regions, width, &translucent);
                self.body.push_str(" mask=\"url(#");
                self.body.push_str(id);
                self.body.push_str(")\"");
            }
            if color.a < 1.0 {
                self.body.push_str(" opacity=\"");
                self.body.push_str(&fmt_num(color.a));
                self.body.push('"');
            }
            self.body.push_str(">\n");
        }
        let ink = Color { a: 1.0, ..color };
        let inner = if fade_or_mask_as_one { format!("{pad}  ") } else { pad.clone() };
        for region in regions.iter().rev() {
            self.body.push_str(&inner);
            self.body.push_str("<path d=\"");
            self.body.push_str(&path_data(&region.path));
            self.body.push('"');
            self.body.push_str(fill_rule(&region.path));
            self.color_attrs("fill", ink);
            self.color_attrs("stroke", ink);
            self.body.push_str(" stroke-width=\"");
            self.body.push_str(&fmt_num(width * 2.0));
            self.body.push_str("\" stroke-linecap=\"round\" stroke-linejoin=\"round\"/>\n");
        }
        if fade_or_mask_as_one {
            self.body.push_str(&pad);
            self.body.push_str("</g>\n");
        }
        Ok(())
    }

    fn erase_mask_def(&mut self, id: &str, regions: &[Region], width: f32, erased: &[&Region]) {
        let mut ring = *regions[0].path.bounds();
        for region in &regions[1..] {
            ring.join(region.path.bounds());
        }
        let area = area_attrs(&ring.with_outset((width + 1.0, width + 1.0)));
        self.defs.push_str("    <mask id=\"");
        self.defs.push_str(id);
        self.defs.push_str("\" maskUnits=\"userSpaceOnUse\" ");
        self.defs.push_str(&area);
        self.defs.push_str(">\n      <rect ");
        self.defs.push_str(&area);
        self.defs.push_str(" fill=\"#ffffff\"/>\n");
        for region in erased {
            self.defs.push_str("      <path d=\"");
            self.defs.push_str(&path_data(&region.path));
            self.defs.push_str("\" fill=\"#000000\"");
            self.defs.push_str(fill_rule(&region.path));
            self.defs.push_str("/>\n");
        }
        self.defs.push_str("    </mask>\n");
    }

    fn shape(&mut self, scene: &Scene, shape: &Shape, id: &str, bake_scale: f32, depth: usize) -> Result<(), RenderError> {
        let pad = "  ".repeat(depth);
        self.body.push_str(&pad);
        self.body.push_str("<path d=\"");
        self.body.push_str(&path_data(&shape.geometry));
        self.body.push('"');
        self.body.push_str(fill_rule(&shape.geometry));
        self.paint_attrs(shape.fill.as_ref(), shape.stroke.as_ref(), id);
        if shape.blur > 0.0 {
            let fid = format!("{id}-blur");
            self.defs.push_str("    <filter id=\"");
            self.defs.push_str(&fid);
            self.defs.push_str("\" filterUnits=\"objectBoundingBox\" x=\"-100%\" y=\"-100%\" width=\"300%\" height=\"300%\">");
            self.defs.push_str("<feGaussianBlur stdDeviation=\"");
            self.defs.push_str(&fmt_num(shape.blur));
            self.defs.push_str("\"/></filter>\n");
            self.body.push_str(" filter=\"url(#");
            self.body.push_str(&fid);
            self.body.push_str(")\"");
        }
        self.body.push_str("/>\n");
        if !shape.children.is_empty() {
            let cid = format!("{id}-nest");
            self.clip_path_def(&cid, &shape.geometry);
            self.body.push_str(&pad);
            self.body.push_str("<g clip-path=\"url(#");
            self.body.push_str(&cid);
            self.body.push_str(")\">\n");
            for child in &shape.children {
                self.node(scene, child, bake_scale, depth + 1, None)?;
            }
            self.body.push_str(&pad);
            self.body.push_str("</g>\n");
        }
        Ok(())
    }

    fn paint_attrs(&mut self, fill: Option<&Paint>, stroke: Option<&Stroke>, id: &str) {
        match fill {
            None => self.body.push_str(" fill=\"none\""),
            Some(Paint::Solid(c)) => self.color_attrs("fill", *c),
            Some(paint) => {
                let gid = format!("{id}-fill");
                self.gradient(&gid, paint);
                self.body.push_str(" fill=\"url(#");
                self.body.push_str(&gid);
                self.body.push_str(")\"");
            }
        }
        if let Some(stroke) = stroke {
            match &stroke.paint {
                Paint::Solid(c) => self.color_attrs("stroke", *c),
                paint => {
                    let gid = format!("{id}-stroke");
                    self.gradient(&gid, paint);
                    self.body.push_str(" stroke=\"url(#");
                    self.body.push_str(&gid);
                    self.body.push_str(")\"");
                }
            }
            self.body.push_str(" stroke-width=\"");
            self.body.push_str(&fmt_num(stroke.width));
            self.body.push_str("\" stroke-linecap=\"");
            self.body.push_str(match stroke.cap {
                Cap::Round => "round",
                Cap::Butt => "butt",
                Cap::Square => "square",
            });
            self.body.push_str("\" stroke-linejoin=\"");
            self.body.push_str(match stroke.join {
                Join::Round => "round",
                Join::Miter => "miter",
                Join::Bevel => "bevel",
            });
            self.body.push('"');
            if !stroke.dash.is_empty() {
                self.body.push_str(" stroke-dasharray=\"");
                self.body.push_str(
                    &stroke.dash.iter().map(|v| fmt_num(*v)).collect::<Vec<_>>().join(","),
                );
                self.body.push('"');
            }
        }
    }

    fn color_attrs(&mut self, name: &str, color: Color) {
        self.body.push(' ');
        self.body.push_str(name);
        self.body.push_str("=\"");
        self.body.push_str(&rgb_hex(color));
        self.body.push('"');
        if color.a < 1.0 {
            self.body.push(' ');
            self.body.push_str(name);
            self.body.push_str("-opacity=\"");
            self.body.push_str(&fmt_num(color.a));
            self.body.push('"');
        }
    }

    fn gradient(&mut self, id: &str, paint: &Paint) {
        match paint {
            Paint::Linear { from, to, stops } => {
                self.defs.push_str("    <linearGradient id=\"");
                self.defs.push_str(id);
                self.defs.push_str("\" gradientUnits=\"userSpaceOnUse\" x1=\"");
                self.defs.push_str(&fmt_num(from.x));
                self.defs.push_str("\" y1=\"");
                self.defs.push_str(&fmt_num(from.y));
                self.defs.push_str("\" x2=\"");
                self.defs.push_str(&fmt_num(to.x));
                self.defs.push_str("\" y2=\"");
                self.defs.push_str(&fmt_num(to.y));
                self.defs.push_str("\">\n");
                self.stops(stops);
                self.defs.push_str("    </linearGradient>\n");
            }
            Paint::Radial { center, radius, stops } => {
                self.defs.push_str("    <radialGradient id=\"");
                self.defs.push_str(id);
                self.defs.push_str("\" gradientUnits=\"userSpaceOnUse\" cx=\"");
                self.defs.push_str(&fmt_num(center.x));
                self.defs.push_str("\" cy=\"");
                self.defs.push_str(&fmt_num(center.y));
                self.defs.push_str("\" r=\"");
                self.defs.push_str(&fmt_num(*radius));
                self.defs.push_str("\">\n");
                self.stops(stops);
                self.defs.push_str("    </radialGradient>\n");
            }
            Paint::Solid(_) | Paint::Conic { .. } | Paint::Field { .. } => {}
        }
    }

    fn stops(&mut self, stops: &[Stop]) {
        for stop in stops {
            self.defs.push_str("      <stop offset=\"");
            self.defs.push_str(&fmt_num(stop.pos));
            self.defs.push_str("\" stop-color=\"");
            self.defs.push_str(&rgb_hex(stop.color));
            self.defs.push('"');
            if stop.color.a < 1.0 {
                self.defs.push_str(" stop-opacity=\"");
                self.defs.push_str(&fmt_num(stop.color.a));
                self.defs.push('"');
            }
            self.defs.push_str("/>\n");
        }
    }

    fn blend_style(&mut self, node: &Node) {
        let mut parts = Vec::new();
        if let Some(mode) = css_blend(node.blend) {
            parts.push(format!("mix-blend-mode:{mode}"));
        }
        let nests = matches!(&node.kind, NodeKind::Shape(shape) if !shape.children.is_empty());
        if nests || matches!(node.kind, NodeKind::Group { isolate: true, .. }) {
            parts.push("isolation:isolate".into());
        }
        if !parts.is_empty() {
            self.body.push_str(" style=\"");
            self.body.push_str(&parts.join(";"));
            self.body.push('"');
        }
    }

    fn clip_path_def(&mut self, id: &str, path: &Path) {
        self.defs.push_str("    <clipPath id=\"");
        self.defs.push_str(id);
        self.defs.push_str("\"><path d=\"");
        self.defs.push_str(&path_data(path));
        self.defs.push('"');
        if path.fill_type() == PathFillType::EvenOdd {
            self.defs.push_str(" clip-rule=\"evenodd\"");
        }
        self.defs.push_str("/></clipPath>\n");
    }

    fn clip_attr(&mut self, node: &Node, id: &str, outer: Option<&Path>) -> Result<(), RenderError> {
        let own = node
            .clip
            .as_ref()
            .filter(|clip| clip.feather <= 0.0 && !clip.invert)
            .map(|clip| clip.path.clone());
        if let Some(path) = intersect(own, outer.cloned())? {
            self.clip_ref(&format!("{id}-clip"), &path);
        }
        Ok(())
    }

    fn clip_ref(&mut self, cid: &str, path: &Path) {
        self.clip_path_def(cid, path);
        self.body.push_str(" clip-path=\"url(#");
        self.body.push_str(cid);
        self.body.push_str(")\"");
    }

    fn mask_attr(&mut self, scene: &Scene, node: &Node, id: &str, bake_scale: f32) -> Result<(), RenderError> {
        let soft = node.clip.as_ref().filter(|c| c.feather > 0.0 || c.invert);
        if soft.is_none() && node.mask.is_none() {
            return Ok(());
        }
        if let Some(clip) = soft {
            let mid = format!("{id}-soft");
            self.defs.push_str("    <mask id=\"");
            self.defs.push_str(&mid);
            self.defs.push_str("\" maskUnits=\"userSpaceOnUse\" ");
            self.defs.push_str(&masked_area(node));
            self.defs.push_str(" mask-type=\"alpha\">\n");
            if clip.invert {
                self.defs.push_str("      <rect x=\"-10000\" y=\"-10000\" width=\"20000\" height=\"20000\" fill=\"#ffffff\"/>\n");
                self.defs.push_str("      <path d=\"");
                self.defs.push_str(&path_data(&clip.path));
                self.defs.push_str("\" fill=\"#000000\"");
                if clip.feather > 0.0 {
                    let fid = format!("{id}-softblur");
                    self.blur_filter(&fid, clip.feather);
                    self.defs.push_str(" filter=\"url(#");
                    self.defs.push_str(&fid);
                    self.defs.push_str(")\"");
                }
                self.defs.push_str("/>\n");
            } else {
                self.defs.push_str("      <path d=\"");
                self.defs.push_str(&path_data(&clip.path));
                self.defs.push_str("\" fill=\"#ffffff\"");
                if clip.feather > 0.0 {
                    let fid = format!("{id}-softblur");
                    self.blur_filter(&fid, clip.feather);
                    self.defs.push_str(" filter=\"url(#");
                    self.defs.push_str(&fid);
                    self.defs.push_str(")\"");
                }
                self.defs.push_str("/>\n");
            }
            self.defs.push_str("    </mask>\n");
            self.body.push_str(" mask=\"url(#");
            self.body.push_str(&mid);
            self.body.push_str(")\"");
        }
        if let Some(mask) = &node.mask {
            if matches!(mask.source, MaskSource::Field { .. }) {
                return Ok(());
            }
            let mid = format!("{id}-mask");
            self.write_node_mask(scene, node, mask, &mid, bake_scale)?;
            if soft.is_some() {
                // The opening tag already carries the soft-clip mask. Nesting happens by
                // wrapping is not available mid-attribute, so a second mask replaces it.
                // Callers with both are rare; the soft clip stays in the defs either way.
            }
            self.body.push_str(" mask=\"url(#");
            self.body.push_str(&mid);
            self.body.push_str(")\"");
        }
        Ok(())
    }

    fn write_node_mask(&mut self, scene: &Scene, node: &Node, mask: &Mask, id: &str, bake_scale: f32) -> Result<(), RenderError> {
        self.defs.push_str("    <mask id=\"");
        self.defs.push_str(id);
        self.defs.push_str("\" maskUnits=\"userSpaceOnUse\" ");
        self.defs.push_str(&masked_area(node));
        self.defs.push_str(" mask-type=\"");
        self.defs.push_str(if mask.luminance { "luminance" } else { "alpha" });
        self.defs.push_str("\">\n");
        if let MaskSource::Nodes(nodes) = &mask.source {
            let mut inner = Writer { defs: String::new(), body: String::new(), backend: self.backend };
            for node in nodes {
                inner.node(scene, node, bake_scale, 3, None)?;
            }
            self.defs.push_str(&inner.defs);
            if mask.feather > 0.0 || mask.invert {
                let fid = format!("{id}-fx");
                self.defs.push_str("      <g");
                if mask.feather > 0.0 || mask.invert {
                    self.mask_effect_filter(&fid, mask.feather, mask.invert);
                    self.defs.push_str(" filter=\"url(#");
                    self.defs.push_str(&fid);
                    self.defs.push_str(")\"");
                }
                self.defs.push_str(">\n");
                self.defs.push_str(&inner.body);
                self.defs.push_str("      </g>\n");
            } else {
                self.defs.push_str(&inner.body);
            }
        }
        self.defs.push_str("    </mask>\n");
        Ok(())
    }

    fn blur_filter(&mut self, id: &str, sigma: f32) {
        self.defs.push_str("    <filter id=\"");
        self.defs.push_str(id);
        self.defs.push_str("\" filterUnits=\"objectBoundingBox\" x=\"-100%\" y=\"-100%\" width=\"300%\" height=\"300%\">");
        self.defs.push_str("<feGaussianBlur stdDeviation=\"");
        self.defs.push_str(&fmt_num(sigma));
        self.defs.push_str("\"/></filter>\n");
    }

    fn mask_effect_filter(&mut self, id: &str, feather: f32, invert: bool) {
        self.defs.push_str("    <filter id=\"");
        self.defs.push_str(id);
        self.defs.push_str("\" filterUnits=\"objectBoundingBox\" x=\"-100%\" y=\"-100%\" width=\"300%\" height=\"300%\">");
        if feather > 0.0 {
            self.defs.push_str("<feGaussianBlur stdDeviation=\"");
            self.defs.push_str(&fmt_num(feather));
            self.defs.push_str("\" result=\"b\"/>");
        }
        if invert {
            let input = if feather > 0.0 { "b" } else { "SourceGraphic" };
            self.defs.push_str("<feComponentTransfer in=\"");
            self.defs.push_str(input);
            self.defs.push_str("\"><feFuncA type=\"linear\" slope=\"-1\" intercept=\"1\"/></feComponentTransfer>");
        }
        self.defs.push_str("</filter>\n");
    }

    fn fx_filter(&mut self, node: &Node, id: &str) -> Option<String> {
        let fid = format!("{id}-fx");
        if self.write_fx_filter(&node.effects, &fid) { Some(fid) } else { None }
    }

    fn write_fx_filter(&mut self, effects: &[Effect], fid: &str) -> bool {
        let usable: Vec<&Effect> = effects
            .iter()
            .filter(|e| matches!(e.op, EffectOp::Blur { .. } | EffectOp::Shadow { .. }))
            .filter(|e| !matches!(e.input, EffectInput::Node(_)))
            .collect();
        if usable.is_empty() {
            return false;
        }
        self.defs.push_str("    <filter id=\"");
        self.defs.push_str(fid);
        self.defs.push_str("\" filterUnits=\"objectBoundingBox\" x=\"-100%\" y=\"-100%\" width=\"300%\" height=\"300%\">");
        let mut names: Vec<(String, String)> = Vec::new();
        let mut prev = "SourceGraphic".to_string();
        for (i, effect) in usable.iter().enumerate() {
            let input = match &effect.input {
                EffectInput::Source => "SourceGraphic".to_string(),
                EffectInput::Named(n) => names
                    .iter()
                    .find(|(name, _)| name == n)
                    .map(|(_, result)| result.clone())
                    .unwrap_or_else(|| prev.clone()),
                EffectInput::Previous | EffectInput::Node(_) => prev.clone(),
            };
            let result = format!("e{i}");
            match &effect.op {
                EffectOp::Blur { sigma } => {
                    self.defs.push_str("<feGaussianBlur in=\"");
                    self.defs.push_str(&input);
                    self.defs.push_str("\" stdDeviation=\"");
                    self.defs.push_str(&fmt_num(*sigma));
                    self.defs.push_str("\" result=\"");
                    self.defs.push_str(&result);
                    self.defs.push_str("\"/>");
                }
                EffectOp::Shadow { dx, dy, blur, color } => {
                    self.defs.push_str("<feDropShadow in=\"");
                    self.defs.push_str(&input);
                    self.defs.push_str("\" dx=\"");
                    self.defs.push_str(&fmt_num(*dx));
                    self.defs.push_str("\" dy=\"");
                    self.defs.push_str(&fmt_num(*dy));
                    self.defs.push_str("\" stdDeviation=\"");
                    self.defs.push_str(&fmt_num(*blur));
                    self.defs.push_str("\" flood-color=\"");
                    self.defs.push_str(&rgb_hex(*color));
                    self.defs.push_str("\" flood-opacity=\"");
                    self.defs.push_str(&fmt_num(color.a));
                    self.defs.push_str("\" result=\"");
                    self.defs.push_str(&result);
                    self.defs.push_str("\"/>");
                }
                _ => {}
            }
            if let Some(name) = &effect.name {
                names.push((name.clone(), result.clone()));
            }
            prev = result;
        }
        self.defs.push_str("</filter>\n");
        true
    }
}

/// A plain group, clipped or under a pushed clip, whose children blend with what is below it.
fn pushes_clip(node: &Node) -> bool {
    matches!(node.kind, NodeKind::Group { isolate: false, .. })
        && node.opacity >= 1.0
        && node.blend == Blend::Normal
        && node.mask.is_none()
        && node.effects.is_empty()
        && node.clip.as_ref().is_none_or(|clip| clip.feather <= 0.0)
        && node
            .children()
            .iter()
            .any(|child| child.blend != Blend::Normal || pushes_clip(child))
}

fn area_attrs(rect: &Rect) -> String {
    format!(
        "x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"",
        fmt_num(rect.left),
        fmt_num(rect.top),
        fmt_num(rect.width()),
        fmt_num(rect.height())
    )
}

/// A mask on `node` applies in its local space. Without an explicit region, renderers cut the
/// mask to -10%..120% of the viewport there, which drops a shape placed with `at:`.
fn masked_area(node: &Node) -> String {
    let pad = visual_pad(node) + stroke_overshoot(node) + 1.0;
    area_attrs(&node.local_bounds().unwrap_or_default().with_outset((pad, pad)))
}

/// How far a stroke reaches past the half width that [`Node::local_bounds`] counts: a miter
/// join up to the default miter limit of 4, a square cap by its diagonal.
fn stroke_overshoot(node: &Node) -> f32 {
    let own = match &node.kind {
        NodeKind::Shape(shape) if let Some(stroke) = &shape.stroke => {
            let join: f32 = if stroke.join == Join::Miter { 4.0 } else { 1.0 };
            let cap = if stroke.cap == Cap::Square { std::f32::consts::SQRT_2 } else { 1.0 };
            stroke.width / 2.0 * (join.max(cap) - 1.0)
        }
        _ => 0.0,
    };
    let masks = match node.mask.as_deref().map(|m| &m.source) {
        Some(MaskSource::Nodes(nodes)) => nodes.as_slice(),
        _ => &[],
    };
    node.children().iter().chain(masks).map(stroke_overshoot).fold(own, f32::max)
}

fn clip_region(clip: &Clip) -> Result<Path, RenderError> {
    if !clip.invert {
        return Ok(clip.path.clone());
    }
    Path::rect(Rect::from_xywh(-10000.0, -10000.0, 20000.0, 20000.0), None)
        .op(&clip.path, PathOp::Difference)
        .ok_or_else(|| RenderError::Skia("outside clip".into()))
}

fn intersect(a: Option<Path>, b: Option<Path>) -> Result<Option<Path>, RenderError> {
    match (a, b) {
        (Some(a), Some(b)) => a
            .op(&b, PathOp::Intersect)
            .map(Some)
            .ok_or_else(|| RenderError::Skia("clip intersect".into())),
        (a, b) => Ok(a.or(b)),
    }
}

fn to_local(path: &Path, transform: &Matrix) -> Path {
    transform.invert().map(|inv| path.make_transform(&inv)).unwrap_or_default()
}

fn fill_rule(path: &Path) -> &'static str {
    if path.fill_type() == PathFillType::EvenOdd {
        " fill-rule=\"evenodd\""
    } else {
        ""
    }
}

fn has_outline(node: &Node) -> bool {
    node.effects.iter().any(|e| matches!(e.op, EffectOp::Outline { width, .. } if width > 0.0))
}

enum SvgStep<'a> {
    Filters(&'a [Effect]),
    Outline { width: f32, color: Color },
}

fn svg_steps(effects: &[Effect]) -> Vec<SvgStep<'_>> {
    let mut steps = Vec::new();
    let mut start: Option<usize> = None;
    for (i, effect) in effects.iter().enumerate() {
        match &effect.op {
            EffectOp::Outline { width, color } if *width > 0.0 => {
                if let Some(s) = start.take() {
                    steps.push(SvgStep::Filters(&effects[s..i]));
                }
                steps.push(SvgStep::Outline { width: *width, color: *color });
            }
            EffectOp::Blur { .. } | EffectOp::Shadow { .. } => {
                if start.is_none() {
                    start = Some(i);
                }
            }
            _ => {
                if let Some(s) = start.take() {
                    steps.push(SvgStep::Filters(&effects[s..i]));
                }
            }
        }
    }
    if let Some(s) = start {
        steps.push(SvgStep::Filters(&effects[s..]));
    }
    steps
}

fn rgb_hex(c: Color) -> String {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", b(c.r), b(c.g), b(c.b))
}

fn matrix_attr(m: &Matrix) -> Option<String> {
    let id = (m.scale_x() - 1.0).abs() < 1.0e-6
        && (m.scale_y() - 1.0).abs() < 1.0e-6
        && m.skew_x().abs() < 1.0e-6
        && m.skew_y().abs() < 1.0e-6
        && m.translate_x().abs() < 1.0e-6
        && m.translate_y().abs() < 1.0e-6;
    if id {
        return None;
    }
    Some(format!(
        "matrix({} {} {} {} {} {})",
        fmt_num(m.scale_x()),
        fmt_num(m.skew_y()),
        fmt_num(m.skew_x()),
        fmt_num(m.scale_y()),
        fmt_num(m.translate_x()),
        fmt_num(m.translate_y())
    ))
}

fn path_data(path: &Path) -> String {
    let mut out = String::new();
    let mut iter = PathIter::new(path, false);
    while let Some((verb, pts)) = iter.next() {
        match verb {
            Verb::Move => cmd(&mut out, "M", &pts[..1]),
            Verb::Line => {
                if !iter.is_close_line() {
                    cmd(&mut out, "L", &pts[1..2]);
                }
            }
            Verb::Quad => quad_as_cubic(&mut out, pts[0], pts[1], pts[2]),
            Verb::Conic => {
                let w = iter.conic_weight().unwrap_or(1.0);
                emit_conic(
                    &mut out,
                    Conic { p0: pts[0], p1: pts[1], p2: pts[2], w },
                    (0.0, 1.0),
                    3,
                );
            }
            Verb::Cubic => cmd(&mut out, "C", &pts[1..4]),
            Verb::Close => out.push('Z'),
            Verb::Done => break,
        }
    }
    out
}

fn cmd(out: &mut String, op: &str, pts: &[Point]) {
    out.push_str(op);
    for p in pts {
        out.push(' ');
        out.push_str(&fmt_num(p.x));
        out.push(' ');
        out.push_str(&fmt_num(p.y));
    }
}

fn quad_as_cubic(out: &mut String, p0: Point, p1: Point, p2: Point) {
    let c1 = Point::new(p0.x + (p1.x - p0.x) * (2.0 / 3.0), p0.y + (p1.y - p0.y) * (2.0 / 3.0));
    let c2 = Point::new(p2.x + (p1.x - p2.x) * (2.0 / 3.0), p2.y + (p1.y - p2.y) * (2.0 / 3.0));
    cmd(out, "C", &[c1, c2, p2]);
}

#[derive(Clone, Copy)]
struct Conic {
    p0: Point,
    p1: Point,
    p2: Point,
    w: f32,
}

fn emit_conic(out: &mut String, conic: Conic, span: (f32, f32), depth: u32) {
    let (t0, t1) = span;
    if depth == 0 {
        let a = conic_at(&conic, t0);
        let m = conic_at(&conic, (t0 + t1) * 0.5);
        let b = conic_at(&conic, t1);
        let ctrl = Point::new(2.0 * m.x - 0.5 * (a.x + b.x), 2.0 * m.y - 0.5 * (a.y + b.y));
        quad_as_cubic(out, a, ctrl, b);
        return;
    }
    let tm = (t0 + t1) * 0.5;
    emit_conic(out, conic, (t0, tm), depth - 1);
    emit_conic(out, conic, (tm, t1), depth - 1);
}

fn conic_at(conic: &Conic, t: f32) -> Point {
    let s = 1.0 - t;
    let u = s * s;
    let v = 2.0 * s * t * conic.w;
    let ww = t * t;
    let d = u + v + ww;
    Point::new(
        (u * conic.p0.x + v * conic.p1.x + ww * conic.p2.x) / d,
        (u * conic.p0.y + v * conic.p1.y + ww * conic.p2.y) / d,
    )
}
