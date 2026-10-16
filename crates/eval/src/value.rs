//! The dynamic value every expression evaluates to, its kinds, and its arithmetic.

use std::fmt;
use std::rc::Rc;

use artscript_ir::Field;

use crate::color::{Ramp, Rgba};
use crate::paint::GradSpec;
use crate::scope::{Component, Module, Palette, RefVal, RuleDef};

/// One coordinate: canvas units or a percent of the current frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Coord {
    Num(f64),
    Pct(f64),
}

impl Coord {
    pub fn pos(self, min: f64, size: f64) -> f64 {
        match self {
            Coord::Num(v) => v,
            Coord::Pct(p) => min + p * size,
        }
    }

    pub fn len(self, size: f64) -> f64 {
        match self {
            Coord::Num(v) => v,
            Coord::Pct(p) => p * size,
        }
    }
}

/// A color that remembers the ramp and index it came from, so `shade` can walk the ramp.
#[derive(Clone, Debug, PartialEq)]
pub struct Col {
    pub rgba: Rgba,
    pub ramp: Option<(Rc<Ramp>, f64)>,
}

impl Col {
    pub fn plain(rgba: Rgba) -> Col {
        Col { rgba, ramp: None }
    }
}

#[derive(Clone)]
pub enum Value {
    None,
    Bool(bool),
    Num(f64),
    Percent(f64),
    Vec2(Coord, Coord),
    Size(Coord, Coord),
    /// A tight comma tuple that is not a pair of numbers, e.g. `p.ink,1.5` or `p.peach.3,.4`.
    Tuple(Vec<Value>),
    Color(Col),
    Ramp(Rc<Ramp>),
    Palette(Rc<Palette>),
    Gradient(Rc<GradSpec>),
    Field(Rc<Field>),
    /// `field > t`, a thresholded field usable as a mask.
    FieldMask(Rc<Field>, f64),
    /// `field -> ramp`, a field mapped through colors.
    FieldPaint(Rc<Field>, Vec<Rgba>),
    Range(f64, f64),
    List(Vec<Value>),
    Str(String),
    Ident(String),
    Component(Rc<Component>),
    Module(Rc<Module>),
    Ref(Rc<RefVal>),
    Rule(Rc<RuleDef>),
}

/// Kind used for parameter type checks (E0320) and messages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    None,
    Bool,
    Number,
    Vector,
    Color,
    Palette,
    Field,
    Range,
    List,
    Str,
    Word,
    Component,
    Module,
    Ref,
    Rule,
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Kind::None => "none",
            Kind::Bool => "a boolean",
            Kind::Number => "a number",
            Kind::Vector => "a vector like 4,2",
            Kind::Color => "a color or paint",
            Kind::Palette => "a palette",
            Kind::Field => "a field",
            Kind::Range => "a range like 0..4",
            Kind::List => "a list",
            Kind::Str => "a string",
            Kind::Word => "a word",
            Kind::Component => "a component",
            Kind::Module => "a module",
            Kind::Ref => "a node reference",
            Kind::Rule => "a rule",
        };
        f.write_str(s)
    }
}

impl Value {
    pub fn kind(&self) -> Kind {
        match self {
            Value::None => Kind::None,
            Value::Bool(_) => Kind::Bool,
            Value::Num(_) | Value::Percent(_) => Kind::Number,
            Value::Vec2(..) | Value::Size(..) => Kind::Vector,
            Value::Tuple(xs) if xs.iter().all(|x| x.kind() == Kind::Number) => Kind::Vector,
            Value::Tuple(_) => Kind::List,
            Value::Color(_) | Value::Ramp(_) | Value::Gradient(_) | Value::FieldPaint(..) => Kind::Color,
            Value::Palette(_) => Kind::Palette,
            Value::Field(_) | Value::FieldMask(..) => Kind::Field,
            Value::Range(..) => Kind::Range,
            Value::List(_) => Kind::List,
            Value::Str(_) => Kind::Str,
            Value::Ident(_) => Kind::Word,
            Value::Component(_) => Kind::Component,
            Value::Module(_) => Kind::Module,
            Value::Ref(_) => Kind::Ref,
            Value::Rule(_) => Kind::Rule,
        }
    }

    /// Whether a parameter whose default has kind `self.kind()` accepts `other`.
    pub fn accepts(&self, other: &Value) -> bool {
        let (a, b) = (self.kind(), other.kind());
        a == b
            || matches!(other, Value::None)
            || matches!(self, Value::None)
            || matches!((a, b), (Kind::Component, Kind::Word) | (Kind::List, Kind::Range) | (Kind::Range, Kind::List))
            || (a == Kind::Color && b == Kind::Field)
            || (a == Kind::Vector && b == Kind::Number)
    }

    pub fn num(&self) -> Option<f64> {
        match self {
            Value::Num(v) => Some(*v),
            Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            _ => None,
        }
    }

    pub fn truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::None => false,
            Value::Num(v) => *v != 0.0,
            _ => true,
        }
    }

    /// Two coordinates from a vector, size, number pair tuple, or a single number (`v,v`).
    pub fn pair(&self) -> Option<(Coord, Coord)> {
        match self {
            Value::Vec2(a, b) | Value::Size(a, b) => Some((*a, *b)),
            Value::Num(v) => Some((Coord::Num(*v), Coord::Num(*v))),
            Value::Percent(p) => Some((Coord::Pct(*p), Coord::Pct(*p))),
            Value::Tuple(xs) | Value::List(xs) if xs.len() == 2 => Some((xs[0].coord()?, xs[1].coord()?)),
            _ => None,
        }
    }

    pub fn coord(&self) -> Option<Coord> {
        match self {
            Value::Num(v) => Some(Coord::Num(*v)),
            Value::Percent(p) => Some(Coord::Pct(*p)),
            _ => None,
        }
    }

    /// A pure number pair.
    pub fn vec2(&self) -> Option<(f64, f64)> {
        match self.pair()? {
            (Coord::Num(a), Coord::Num(b)) => Some((a, b)),
            _ => None,
        }
    }

    pub fn items(&self) -> Option<Vec<Value>> {
        match self {
            Value::List(xs) | Value::Tuple(xs) => Some(xs.clone()),
            Value::Range(a, b) => {
                let (a, b) = (*a, *b);
                let n = (b - a).ceil().max(0.0) as usize;
                Some((0..n).map(|i| Value::Num(a + i as f64)).collect())
            }
            _ => None,
        }
    }

    pub fn word(&self) -> Option<&str> {
        match self {
            Value::Ident(s) | Value::Str(s) => Some(s),
            _ => None,
        }
    }

    /// A color, taking a ramp as its base stop.
    pub fn color(&self) -> Option<Col> {
        match self {
            Value::Color(c) => Some(c.clone()),
            Value::Ramp(r) => Some(Col { rgba: r.base_color(), ramp: Some((r.clone(), r.base as f64)) }),
            _ => None,
        }
    }

    /// Builds the canonical value for a tight tuple: number pairs become vectors.
    pub fn tuple(xs: Vec<Value>) -> Value {
        if xs.len() == 2
            && let (Some(a), Some(b)) = (xs[0].coord(), xs[1].coord())
        {
            return Value::Vec2(a, b);
        }
        Value::Tuple(xs)
    }

    /// Short human form for notes and `explain`.
    pub fn show(&self) -> String {
        match self {
            Value::None => "none".into(),
            Value::Bool(b) => b.to_string(),
            Value::Num(v) => num(*v),
            Value::Percent(p) => format!("{}%", num(p * 100.0)),
            Value::Vec2(a, b) => format!("{},{}", coord(*a), coord(*b)),
            Value::Size(a, b) => format!("{}x{}", coord(*a), coord(*b)),
            Value::Tuple(xs) => xs.iter().map(Value::show).collect::<Vec<_>>().join(","),
            Value::List(xs) => format!("[{}]", xs.iter().map(Value::show).collect::<Vec<_>>().join(", ")),
            Value::Color(c) => c.rgba.to_hex(),
            Value::Ramp(r) => format!("ramp {}", r.base_color().to_hex()),
            Value::Palette(p) => format!("palette {}", p.name),
            Value::Gradient(_) => "gradient".into(),
            Value::Field(_) | Value::FieldMask(..) | Value::FieldPaint(..) => "field".into(),
            Value::Range(a, b) => format!("{}..{}", num(*a), num(*b)),
            Value::Str(s) => format!("{s:?}"),
            Value::Ident(s) => s.clone(),
            Value::Component(c) => c.name.clone(),
            Value::Module(m) => m.path.clone(),
            Value::Ref(r) => format!("@{}", r.label),
            Value::Rule(r) => r.name.clone(),
        }
    }
}

impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.show())
    }
}

fn coord(c: Coord) -> String {
    match c {
        Coord::Num(v) => num(v),
        Coord::Pct(p) => format!("{}%", num(p * 100.0)),
    }
}

/// Numbers in messages: at most two decimals, no trailing zeros.
pub fn num(v: f64) -> String {
    let r = (v * 100.0).round() / 100.0;
    let s = format!("{r:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.to_string() }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arith {
    Add,
    Sub,
    Mul,
    Div,
}

impl Arith {
    fn apply(self, a: f64, b: f64) -> f64 {
        match self {
            Arith::Add => a + b,
            Arith::Sub => a - b,
            Arith::Mul => a * b,
            Arith::Div => a / b,
        }
    }

    fn field(self) -> artscript_ir::FieldOp {
        match self {
            Arith::Add => artscript_ir::FieldOp::Add,
            Arith::Sub => artscript_ir::FieldOp::Sub,
            Arith::Mul => artscript_ir::FieldOp::Mul,
            Arith::Div => artscript_ir::FieldOp::Div,
        }
    }
}

fn coord_op(op: Arith, a: Coord, b: Coord) -> Option<Coord> {
    use Coord::*;
    Some(match (a, b) {
        (Num(x), Num(y)) => Num(op.apply(x, y)),
        (Pct(x), Pct(y)) if matches!(op, Arith::Add | Arith::Sub) => Pct(op.apply(x, y)),
        (Pct(x), Num(y)) if matches!(op, Arith::Mul | Arith::Div) => Pct(op.apply(x, y)),
        (Num(x), Pct(y)) if op == Arith::Mul => Pct(x * y),
        _ => return None,
    })
}

fn as_field(v: &Value) -> Option<Field> {
    match v {
        Value::Field(f) => Some((**f).clone()),
        Value::Num(n) => Some(Field::Const(*n as f32)),
        _ => None,
    }
}

/// Elementwise arithmetic on numbers, percents, vectors, lists and fields.
pub fn arith(op: Arith, a: &Value, b: &Value) -> Result<Value, String> {
    use Value::*;
    let fail = || format!("cannot apply `{}` to {} and {}", sym(op), a.kind(), b.kind());
    Ok(match (a, b) {
        (Num(x), Num(y)) => Num(op.apply(*x, *y)),
        (Percent(_), _) | (_, Percent(_)) if a.coord().is_some() && b.coord().is_some() => {
            match coord_op(op, a.coord().unwrap(), b.coord().unwrap()).ok_or_else(fail)? {
                Coord::Num(v) => Num(v),
                Coord::Pct(p) => Percent(p),
            }
        }
        (Field(_), _) | (_, Field(_)) => {
            let (fa, fb) = (as_field(a).ok_or_else(fail)?, as_field(b).ok_or_else(fail)?);
            Field(Rc::new(artscript_ir::Field::Binary(op.field(), Box::new(fa), Box::new(fb))))
        }
        (List(xs), Num(_)) | (Tuple(xs), Num(_)) => {
            List(xs.iter().map(|x| arith(op, x, b)).collect::<Result<_, _>>()?)
        }
        (Num(_), List(ys)) | (Num(_), Tuple(ys)) if op == Arith::Mul => {
            List(ys.iter().map(|y| arith(op, a, y)).collect::<Result<_, _>>()?)
        }
        _ => {
            let (pa, pb) = (a.pair().ok_or_else(fail)?, b.pair().ok_or_else(fail)?);
            let x = coord_op(op, pa.0, pb.0).ok_or_else(fail)?;
            let y = coord_op(op, pa.1, pb.1).ok_or_else(fail)?;
            if matches!(a, Size(..)) { Size(x, y) } else { Vec2(x, y) }
        }
    })
}

fn sym(op: Arith) -> &'static str {
    match op {
        Arith::Add => "+",
        Arith::Sub => "-",
        Arith::Mul => "*",
        Arith::Div => "/",
    }
}

pub fn equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Num(x), Value::Num(y)) => x == y,
        (Value::Percent(x), Value::Percent(y)) => x == y,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::None, Value::None) => true,
        (Value::Color(x), Value::Color(y)) => x.rgba == y.rgba,
        (Value::Component(x), Value::Component(y)) => Rc::ptr_eq(x, y),
        _ => match (a.word(), b.word()) {
            (Some(x), Some(y)) => x == y,
            _ => match (a.items(), b.items()) {
                (Some(xs), Some(ys)) => xs.len() == ys.len() && xs.iter().zip(&ys).all(|(x, y)| equal(x, y)),
                _ => match (a.vec2(), b.vec2()) {
                    (Some(x), Some(y)) => x == y,
                    _ => false,
                },
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_scale_elementwise() {
        let l = Value::List(vec![Value::Num(2.0), Value::Num(4.0)]);
        let out = arith(Arith::Mul, &l, &Value::Num(1.5)).unwrap();
        assert_eq!(out.show(), "[3, 6]");
        assert_eq!(arith(Arith::Div, &l, &Value::Num(2.0)).unwrap().show(), "[1, 2]");
    }

    #[test]
    fn vectors_add_and_scale() {
        let v = Value::Vec2(Coord::Num(516.0), Coord::Num(598.0));
        let w = Value::Vec2(Coord::Num(4.0), Coord::Num(-2.0));
        assert_eq!(arith(Arith::Add, &v, &w).unwrap().show(), "520,596");
        assert_eq!(arith(Arith::Mul, &w, &Value::Num(3.0)).unwrap().show(), "12,-6");
        assert_eq!(arith(Arith::Mul, &w, &w).unwrap().show(), "16,4");
    }

    #[test]
    fn percents_stay_percents_under_scaling() {
        let out = arith(Arith::Mul, &Value::Percent(0.5), &Value::Num(0.5)).unwrap();
        assert_eq!(out.show(), "25%");
    }
}
