//! Plain geometry the IR stores. A backend reads these as data and computes with its own kernel;
//! `ramose-geom` converts them to Skia.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const fn new(x: f32, y: f32) -> Point {
        Point { x, y }
    }
}

/// Edges in artboard units, y down. `left <= right` and `top <= bottom` for a non-empty rect.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Rect {
    pub const fn new(left: f32, top: f32, right: f32, bottom: f32) -> Rect {
        Rect { left, top, right, bottom }
    }
}

/// A 3x3 projective matrix, row by row: `[scale_x, skew_x, trans_x, skew_y, scale_y, trans_y,
/// persp_0, persp_1, persp_2]`. It maps the column `(x, y, 1)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matrix(pub [f32; 9]);

impl Matrix {
    pub const IDENTITY: Matrix = Matrix([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
}

impl Default for Matrix {
    fn default() -> Matrix {
        Matrix::IDENTITY
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FillType {
    #[default]
    Winding,
    EvenOdd,
    InverseWinding,
    InverseEvenOdd,
}

/// Each verb reads the next points in order: `Move` and `Line` one, `Quad` and `Conic` two (with
/// one weight for a conic), `Cubic` three, `Close` none. Every contour starts with `Move`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verb {
    Move,
    Line,
    Quad,
    Conic,
    Cubic,
    Close,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Path {
    pub fill: FillType,
    pub verbs: Vec<Verb>,
    pub points: Vec<Point>,
    /// One per `Conic` verb, in order.
    pub weights: Vec<f32>,
    /// What the path was built as when it is exactly an oval or a rounded rect. A backend may
    /// draw it analytically. `verbs`, `points` and `weights` are always the full outline.
    pub primitive: Option<Primitive>,
}

/// An oval or a rounded rect as a path traces it: clockwise unless `ccw`, from the point
/// numbered `start` (0 to 3 for an oval, 0 to 7 for a rounded rect).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Primitive {
    Oval { rect: Rect, ccw: bool, start: u8 },
    /// `radii` are the corner radii from the top-left, clockwise.
    RRect { rect: Rect, radii: [Point; 4], ccw: bool, start: u8 },
}

impl Path {
    pub fn is_empty(&self) -> bool {
        self.verbs.is_empty()
    }
}
