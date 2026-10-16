//! Scene IR backends: Skia CPU raster (PNG) and artscript's own SVG writer.

use artscript_ir::{Profile, Scene};

#[derive(Debug, Clone, PartialEq)]
pub struct Unsupported {
    /// Node path that uses the feature.
    pub node: String,
    /// Short feature name, e.g. `field fill`, `fx grain`.
    pub feature: String,
}

#[derive(Debug)]
pub enum RenderError {
    /// Under `Profile::Vector`, every node that uses a raster-only feature.
    NotVector(Vec<Unsupported>),
    Skia(String),
}

/// Renders the scene at `scale` (1 = one pixel per canvas unit) and encodes it as PNG.
/// Deterministic: the same scene produces the same bytes.
pub fn render_png(scene: &Scene, scale: f32) -> Result<Vec<u8>, RenderError> {
    let _ = (scene, scale);
    todo!()
}

/// Writes SVG. Under `Profile::Svg`, subtrees using raster-only features are baked to
/// embedded PNG tiles rendered at `bake_scale`. Under `Profile::Vector` they are an error.
pub fn render_svg(scene: &Scene, profile: Profile, bake_scale: f32) -> Result<String, RenderError> {
    let _ = (scene, profile, bake_scale);
    todo!()
}

/// Every node that would bake under `Profile::Svg` (or fail under `Profile::Vector`).
pub fn raster_only_features(scene: &Scene) -> Vec<Unsupported> {
    let _ = scene;
    todo!()
}
