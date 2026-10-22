//! Where a frame sits on the artboard: the affine steps and the warps of every ancestor, in
//! application order. A warped ancestor's warp is known only when its body closes, so a place
//! inside it holds a cell that the ancestor fills once, and every record placed inside shares
//! that cell. Until the fill, the cell reads as no warp, which is the flat frame the parts of a
//! warped node lay out in and read each other in. A warp is never inverted: artboard points come
//! back into a frame through its affine steps only.

use std::cell::OnceCell;
use std::rc::Rc;

use ramose_geom::{ToSk, WarpGeom};
use ramose_ir::{Node, Warp};
use skia_safe::{Matrix, Path, Point, Rect};

#[derive(Clone)]
pub(crate) struct Place(Vec<Link>);

#[derive(Clone)]
enum Link {
    Affine(Matrix),
    Warp(Rc<OnceCell<Warp>>),
}

impl Place {
    pub fn identity() -> Place {
        Place(vec![Link::Affine(Matrix::new_identity())])
    }

    /// `m` applied before everything here: a child frame placed by `m` in this one.
    pub fn inner(&self, m: Matrix) -> Place {
        let mut links = self.0.clone();
        match links.first_mut() {
            Some(Link::Affine(a)) => *a = *a * m,
            _ => links.insert(0, Link::Affine(m)),
        }
        Place(links)
    }

    /// A warp applied before everything here, and the cell that fills it.
    pub fn inner_warp(&self) -> (Place, Rc<OnceCell<Warp>>) {
        let cell = Rc::new(OnceCell::new());
        let mut links = Vec::with_capacity(self.0.len() + 1);
        links.push(Link::Warp(cell.clone()));
        links.extend(self.0.iter().cloned());
        (Place(links), cell)
    }

    /// `node`'s content frame, with this as its parent frame: its resolved warp, then its
    /// transform.
    pub fn inner_node(&self, node: &Node) -> Place {
        let mut place = self.inner(node.transform.to_sk());
        if !node.warp.is_none() {
            place
                .0
                .insert(0, Link::Warp(Rc::new(OnceCell::from(node.warp.clone()))));
        }
        place
    }

    /// Moves the frame `base` placed by `t`, in `base`'s units, for a `grid` that learns its
    /// pitch from a copy it already placed. `base`'s links end this chain, with its first one
    /// folded into the step this chain applies just inside it, so that step is the one to move.
    pub fn shift(&mut self, base: &Place, t: &Matrix) {
        let k = self.0.len() - base.0.len();
        match (&base.0[0], &mut self.0[k]) {
            (Link::Affine(b), Link::Affine(a)) => {
                if let Some(inv) = b.invert() {
                    *a = *b * *t * inv * *a;
                }
            }
            (Link::Warp(_), _) => self.0.insert(k, Link::Affine(*t)),
            (Link::Affine(_), Link::Warp(_)) => {
                unreachable!("a record's chain ends with the links of the scope that placed it")
            }
        }
    }

    /// The warps in order, each with the affine product applied before it, and the affine
    /// product after the last. An unfilled warp is the identity, so it joins its neighbours.
    fn runs(&self) -> (Vec<(Matrix, Warp)>, Matrix) {
        let mut runs = Vec::new();
        let mut pending = Matrix::new_identity();
        for link in &self.0 {
            match link {
                Link::Affine(m) => pending = *m * pending,
                Link::Warp(cell) => {
                    if let Some(w) = cell.get().filter(|w| !w.is_none()) {
                        runs.push((pending, w.clone()));
                        pending = Matrix::new_identity();
                    }
                }
            }
        }
        (runs, pending)
    }

    /// The affine product with every warp skipped: the frame the content lays out in.
    pub fn flat(&self) -> Matrix {
        self.0
            .iter()
            .fold(Matrix::new_identity(), |m, link| match link {
                Link::Affine(a) => *a * m,
                Link::Warp(_) => m,
            })
    }

    /// `None` for a point a warp hides.
    pub fn map_point(&self, p: Point) -> Option<Point> {
        let (runs, tail) = self.runs();
        let p = runs
            .iter()
            .try_fold(p, |p, (m, w)| w.map_point(m.map_point(p)))?;
        Some(tail.map_point(p))
    }

    pub fn map_rect(&self, r: Rect) -> Rect {
        self.map_rect_then(r, &Matrix::new_identity())
    }

    /// The box of `r` on the artboard and then under `after`, with the affine steps between two
    /// warps composed before they map, so a chain with no warp maps the box once.
    pub fn map_rect_then(&self, r: Rect, after: &Matrix) -> Rect {
        let (runs, tail) = self.runs();
        let r = runs.iter().fold(r, |r, (m, w)| w.map_rect(m.map_rect(r).0));
        (*after * tail).map_rect(r).0
    }

    /// `tol` is in artboard units; each warp flattens to it in its own frame.
    pub fn map_path(&self, path: &Path, tol: f32) -> Path {
        let (runs, tail) = self.runs();
        let mut scale_after = vec![axis_scale(&tail) as f32];
        for (m, _) in runs.iter().skip(1).rev() {
            let last = scale_after[scale_after.len() - 1];
            scale_after.push(last * axis_scale(m) as f32);
        }
        scale_after.reverse();
        let mapped = runs
            .iter()
            .zip(scale_after)
            .fold(path.clone(), |p, ((m, w), scale)| {
                w.map_path(&p.with_transform(m), tol / scale.max(1e-6))
            });
        mapped.with_transform(&tail)
    }

    /// Canvas back into this frame through its affine steps: a reader inside a warp reads the
    /// flat frame.
    pub fn unmap_point(&self, p: Point) -> Point {
        inverse(&self.flat()).map_point(p)
    }

    pub fn unmap_path(&self, path: &Path) -> Path {
        path.with_transform(&inverse(&self.flat()))
    }
}

pub(crate) fn inverse(m: &Matrix) -> Matrix {
    m.invert().unwrap_or_default()
}

/// Parent units per local unit, along the more-stretched axis.
pub(crate) fn axis_scale(m: &Matrix) -> f64 {
    let sx = f64::from(m.scale_x()).hypot(f64::from(m.skew_y()));
    let sy = f64::from(m.scale_y()).hypot(f64::from(m.skew_x()));
    sx.max(sy)
}
