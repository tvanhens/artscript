//! Lossless conversion between the IR's geometry and Skia's.

use ramose_ir as ir;
use skia_safe as sk;
use skia_safe::rrect::Corner;

pub trait ToSk {
    type Sk;
    fn to_sk(&self) -> Self::Sk;
}

pub trait ToIr {
    type Ir;
    fn to_ir(&self) -> Self::Ir;
}

impl ToSk for ir::Point {
    type Sk = sk::Point;
    fn to_sk(&self) -> sk::Point {
        sk::Point::new(self.x, self.y)
    }
}

impl ToIr for sk::Point {
    type Ir = ir::Point;
    fn to_ir(&self) -> ir::Point {
        ir::Point::new(self.x, self.y)
    }
}

impl ToSk for ir::Rect {
    type Sk = sk::Rect;
    fn to_sk(&self) -> sk::Rect {
        sk::Rect::new(self.left, self.top, self.right, self.bottom)
    }
}

impl ToIr for sk::Rect {
    type Ir = ir::Rect;
    fn to_ir(&self) -> ir::Rect {
        ir::Rect::new(self.left, self.top, self.right, self.bottom)
    }
}

impl ToSk for ir::Matrix {
    type Sk = sk::Matrix;
    fn to_sk(&self) -> sk::Matrix {
        let mut m = sk::Matrix::new_identity();
        m.set_9(&self.0);
        m
    }
}

impl ToIr for sk::Matrix {
    type Ir = ir::Matrix;
    fn to_ir(&self) -> ir::Matrix {
        let mut m = [0.0; 9];
        self.get_9(&mut m);
        ir::Matrix(m)
    }
}

impl ToSk for ir::Path {
    type Sk = sk::Path;
    fn to_sk(&self) -> sk::Path {
        let fill = fill_to_sk(self.fill);
        let mut path = match self.primitive {
            Some(p) => primitive_path(p),
            None => {
                let points: Vec<sk::Point> = self.points.iter().map(ToSk::to_sk).collect();
                let verbs: Vec<sk::PathVerb> = self.verbs.iter().map(|&v| verb_to_sk(v)).collect();
                return sk::Path::raw(&points, &verbs, &self.weights, fill, None);
            }
        };
        if path.fill_type() != fill {
            path.set_fill_type(fill);
        }
        path
    }
}

impl ToIr for sk::Path {
    type Ir = ir::Path;
    fn to_ir(&self) -> ir::Path {
        ir::Path {
            fill: fill_to_ir(self.fill_type()),
            verbs: self.verbs().iter().map(|&v| verb_to_ir(v)).collect(),
            points: self.points().iter().map(ToIr::to_ir).collect(),
            weights: self.conic_weights().to_vec(),
            primitive: primitive_of(self),
        }
    }
}

/// The oval or rounded rect `path` was built as, when rebuilding it gives the same outline, so
/// [`ToSk`] hands Skia back a path it still draws analytically.
fn primitive_of(path: &sk::Path) -> Option<ir::Primitive> {
    let candidates: Vec<ir::Primitive> = if let Some(rect) = path.is_oval() {
        let rect = rect.to_ir();
        [false, true]
            .into_iter()
            .flat_map(|ccw| (0..4).map(move |start| ir::Primitive::Oval { rect, ccw, start }))
            .collect()
    } else {
        let rr = path.is_rrect()?;
        let rect = rr.rect().to_ir();
        let radii = [Corner::UpperLeft, Corner::UpperRight, Corner::LowerRight, Corner::LowerLeft]
            .map(|c| rr.radii(c).to_ir());
        [false, true]
            .into_iter()
            .flat_map(|ccw| (0..8).map(move |start| ir::Primitive::RRect { rect, radii, ccw, start }))
            .collect()
    };
    candidates.into_iter().find(|&p| {
        let built = primitive_path(p);
        built.verbs() == path.verbs() && built.points() == path.points() && built.conic_weights() == path.conic_weights()
    })
}

fn primitive_path(p: ir::Primitive) -> sk::Path {
    let dir = |ccw: bool| if ccw { sk::PathDirection::CCW } else { sk::PathDirection::CW };
    match p {
        ir::Primitive::Oval { rect, ccw, start } => sk::Path::oval_with_start_index(rect.to_sk(), dir(ccw), start.into()),
        ir::Primitive::RRect { rect, radii, ccw, start } => {
            let rr = sk::RRect::new_rect_radii(rect.to_sk(), &radii.map(|r| r.to_sk()));
            sk::Path::rrect_with_start_index(rr, dir(ccw), start.into())
        }
    }
}

fn verb_to_sk(v: ir::Verb) -> sk::PathVerb {
    match v {
        ir::Verb::Move => sk::PathVerb::Move,
        ir::Verb::Line => sk::PathVerb::Line,
        ir::Verb::Quad => sk::PathVerb::Quad,
        ir::Verb::Conic => sk::PathVerb::Conic,
        ir::Verb::Cubic => sk::PathVerb::Cubic,
        ir::Verb::Close => sk::PathVerb::Close,
    }
}

fn verb_to_ir(v: sk::PathVerb) -> ir::Verb {
    match v {
        sk::PathVerb::Move => ir::Verb::Move,
        sk::PathVerb::Line => ir::Verb::Line,
        sk::PathVerb::Quad => ir::Verb::Quad,
        sk::PathVerb::Conic => ir::Verb::Conic,
        sk::PathVerb::Cubic => ir::Verb::Cubic,
        sk::PathVerb::Close => ir::Verb::Close,
    }
}

fn fill_to_sk(f: ir::FillType) -> sk::PathFillType {
    match f {
        ir::FillType::Winding => sk::PathFillType::Winding,
        ir::FillType::EvenOdd => sk::PathFillType::EvenOdd,
        ir::FillType::InverseWinding => sk::PathFillType::InverseWinding,
        ir::FillType::InverseEvenOdd => sk::PathFillType::InverseEvenOdd,
    }
}

fn fill_to_ir(f: sk::PathFillType) -> ir::FillType {
    match f {
        sk::PathFillType::Winding => ir::FillType::Winding,
        sk::PathFillType::EvenOdd => ir::FillType::EvenOdd,
        sk::PathFillType::InverseWinding => ir::FillType::InverseWinding,
        sk::PathFillType::InverseEvenOdd => ir::FillType::InverseEvenOdd,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trips(path: &sk::Path) -> ir::Path {
        let ir = path.to_ir();
        let back = ir.to_sk();
        assert_eq!(back.verbs(), path.verbs());
        assert_eq!(back.points(), path.points());
        assert_eq!(back.conic_weights(), path.conic_weights());
        assert_eq!(back.fill_type(), path.fill_type());
        assert_eq!(back.is_oval(), path.is_oval());
        assert_eq!(back.is_rrect(), path.is_rrect());
        ir
    }

    #[test]
    fn an_oval_and_a_rounded_rect_come_back_as_skia_built_them() {
        let rect = sk::Rect::new(-3.0, -2.0, 5.0, 4.0);
        for dir in [sk::PathDirection::CW, sk::PathDirection::CCW] {
            for start in 0..4 {
                let oval = round_trips(&sk::Path::oval_with_start_index(rect, dir, start));
                assert!(matches!(oval.primitive, Some(ir::Primitive::Oval { .. })));
            }
            for start in 0..8 {
                let radii = [(1.0, 1.0), (2.0, 0.5), (0.5, 0.5), (1.5, 1.0)].map(sk::Vector::from);
                let rr = sk::RRect::new_rect_radii(rect, &radii);
                let rr = round_trips(&sk::Path::rrect_with_start_index(rr, dir, start));
                assert!(matches!(rr.primitive, Some(ir::Primitive::RRect { .. })));
            }
        }
        let mut inverse = sk::Path::circle((1.0, 1.0), 3.0, None);
        inverse.set_fill_type(sk::PathFillType::InverseEvenOdd);
        round_trips(&inverse);
    }

    #[test]
    fn a_general_path_keeps_every_verb_point_and_weight() {
        let mut b = sk::PathBuilder::new();
        b.move_to((0.0, 0.0))
            .line_to((4.0, 0.0))
            .quad_to((5.0, 1.0), (4.0, 2.0))
            .conic_to((3.0, 4.0), (1.0, 3.0), 0.7)
            .cubic_to((0.0, 3.0), (-1.0, 1.0), (0.0, 0.0))
            .close()
            .move_to((10.0, 10.0))
            .line_to((12.0, 10.0));
        let ir = round_trips(&b.detach());
        assert_eq!(ir.primitive, None);
        assert_eq!(ir.weights, vec![0.7]);
    }

    #[test]
    fn a_matrix_keeps_all_nine_members() {
        let m = sk::Matrix::new_all(1.5, 0.2, 3.0, -0.4, 0.9, -7.0, 0.001, 0.002, 1.1);
        let back = m.to_ir().to_sk();
        let (mut a, mut b) = ([0.0; 9], [0.0; 9]);
        m.get_9(&mut a);
        back.get_9(&mut b);
        assert_eq!(a, b);
    }
}
