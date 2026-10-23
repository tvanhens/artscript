//! The typed IR of a checked brush body: SSA statements with structured `if` and reduction
//! blocks. It is the contract every backend prints, and it names no syntax.

use crate::lang::{Builtin, Field, Noise, Reduce, Ty};

/// One SSA value. Locals are numbered in definition order across the whole body, so a printer
/// names them `v{n}` and two builds print the same text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Local(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LoopVar(pub u32);

/// A checked body: everything a backend needs.
#[derive(Clone, Debug, PartialEq)]
pub struct Body {
    /// Brush parameters in head order. The uniform order and the `Stage::uniforms` order.
    pub params: Vec<(String, Ty)>,
    /// The salts noise calls use, sorted and unique. Index `i` is uniform `u_salt{i}`.
    pub salts: Vec<u32>,
    pub stmts: Vec<Stmt>,
    pub out: Out,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Out {
    pub rgb: Local,
    pub cover: Cover,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cover {
    Value(Local),
    /// No `cover:`. The backend prints the surface's own: `cover(m.edge)` on 2D. It is not a
    /// read of `edge`, so it does not make the body 2D-only.
    Surface,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stmt {
    pub dst: Local,
    pub ty: Ty,
    pub op: Op,
}

/// A nested sequence that ends in one value: an `if` arm or a reduction's item.
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub value: Local,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    /// The f64 as written. Printed with `{}` so the target parses the author's own decimal.
    Num(f64),
    Bool(bool),
    Param(u16),
    Field(Field),
    /// A loop variable, as a number.
    Loop(LoopVar),
    Neg(Local),
    Not(Local),
    /// Arithmetic per component, with a number broadcast to the other side's lanes.
    Arith(Arith, Local, Local),
    /// `< <= > >= == !=` on numbers, `== !=` and `and or` on bools.
    Logic(Logic, Local, Local),
    /// `.x .y .z .xy .r .g .b .a .rgb`. Lane indices in the source's layout.
    Swizzle(Local, Swizzle),
    /// A tuple: numbers and vectors joined into a vec2 or vec3.
    Pack(Vec<Local>),
    Call(Builtin, Vec<Local>),
    /// Self-seeding noise: the backend adds `seed`'s offset to `at` before the call.
    Noise { f: Noise, at: Local, seed: NoiseSeed, oct: u8 },
    /// Both arms are blocks, so only the taken arm runs.
    If { cond: Local, then: Block, otherwise: Block },
    /// `max([each x in lo..hi -> item])`: a loop with constant bounds. `lo < hi`, and the product
    /// of trip counts of nested reductions is at most `MAX_TRIPS`, both checked before this is
    /// built.
    Reduce { op: Reduce, var: LoopVar, lo: i32, hi: i32, item: Block },
    /// `max([a b c])`: a literal list folds left with the same op.
    Fold { op: Reduce, items: Vec<Local> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arith {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Logic {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    And,
    Or,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Swizzle {
    pub lanes: [u8; 3],
    pub len: u8,
}

impl Swizzle {
    /// The member name, or `None` when `name` does not read `ty`.
    pub fn parse(ty: Ty, name: &str) -> Option<Swizzle> {
        let lane = |c: char| match (ty, c) {
            (Ty::Vec2 | Ty::Vec3, 'x') | (Ty::Color, 'r') => Some(0u8),
            (Ty::Vec2 | Ty::Vec3, 'y') | (Ty::Color, 'g') => Some(1),
            (Ty::Vec3, 'z') | (Ty::Color, 'b') => Some(2),
            (Ty::Color, 'a') => Some(3),
            _ => None,
        };
        let allowed = match ty {
            Ty::Vec2 => &["x", "y"][..],
            Ty::Vec3 => &["x", "y", "z", "xy"],
            Ty::Color => &["r", "g", "b", "a", "rgb"],
            Ty::Num | Ty::Bool => &[],
        };
        if !allowed.contains(&name) {
            return None;
        }
        let mut lanes = [0u8; 3];
        for (i, c) in name.chars().enumerate() {
            lanes[i] = lane(c)?;
        }
        Some(Swizzle { lanes, len: name.len() as u8 })
    }

    pub fn ty(self) -> Ty {
        Ty::of_lanes(self.len).expect("a swizzle has one to three lanes")
    }
}

/// Which offset a noise call adds to its point. All of them are computed on the CPU.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NoiseSeed {
    /// The stage's offset: uniform `u_seedo`.
    Stage,
    /// `salt:k`: the salt itself. Its position in `Body::salts` is the uniform `u_salt{i}`.
    Salt(u32),
    /// `seed:n`: a constant offset, `Seeds::from(n).offset`, printed as a literal.
    /// `seed:0` is `(0,0)` and the backend adds nothing.
    Fixed([u8; 2]),
}
