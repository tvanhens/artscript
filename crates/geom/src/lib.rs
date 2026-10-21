//! The IR's geometry computed with Skia: lossless conversion between the two, and the bounds,
//! placement and warp queries eval and render share. The IR itself stays free of Skia.

mod convert;
mod place;
mod warp;

pub use convert::{ToIr, ToSk};
pub use place::{Find, NodeGeom, ShapeGeom, transform_scale, union_bounds};
pub use warp::{Placer, WarpError, WarpGeom};
