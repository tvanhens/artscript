//! The shading language's vocabulary: value types, the fields of the sample `m`, the builtins,
//! and the names a body may not take. Every table here is the single list the checker, the
//! backends, and the docs test read.

/// A value's type in a body. A list is not a type: it exists only inside a reduction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Ty {
    Num,
    Vec2,
    Vec3,
    Color,
    Bool,
}

impl Ty {
    pub fn lanes(self) -> u8 {
        match self {
            Ty::Num | Ty::Bool => 1,
            Ty::Vec2 => 2,
            Ty::Vec3 => 3,
            Ty::Color => 4,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Ty::Num => "a number",
            Ty::Vec2 => "a vec2",
            Ty::Vec3 => "a vec3",
            Ty::Color => "a color",
            Ty::Bool => "a bool",
        }
    }

    pub fn is_vector(self) -> bool {
        matches!(self, Ty::Vec2 | Ty::Vec3)
    }

    /// A number or a vector: what arithmetic and the per-component builtins take.
    pub fn is_numeric(self) -> bool {
        matches!(self, Ty::Num | Ty::Vec2 | Ty::Vec3)
    }

    /// The vector of `lanes` components, or the number for one.
    pub fn of_lanes(lanes: u8) -> Option<Ty> {
        match lanes {
            1 => Some(Ty::Num),
            2 => Some(Ty::Vec2),
            3 => Some(Ty::Vec3),
            _ => None,
        }
    }
}

/// Where a field can be read. Ordered: a body's requirement is the max over its reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Surface {
    Any,
    TwoD,
    Raster,
}

/// A field of `m`, the surface sample.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Field {
    Position,
    Normal,
    Uv,
    Size,
    Paint,
    Seed,
    Px,
    Light,
    Edge,
    EdgeNormal,
    S,
    D,
    Pressure,
    Angle,
    Tilt,
    Velocity,
    Below,
}

impl Field {
    pub const ALL: &'static [Field] = &[
        Field::Position,
        Field::Normal,
        Field::Uv,
        Field::Size,
        Field::Paint,
        Field::Seed,
        Field::Px,
        Field::Light,
        Field::Edge,
        Field::EdgeNormal,
        Field::S,
        Field::D,
        Field::Pressure,
        Field::Angle,
        Field::Tilt,
        Field::Velocity,
        Field::Below,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Field::Position => "position",
            Field::Normal => "normal",
            Field::Uv => "uv",
            Field::Size => "size",
            Field::Paint => "paint",
            Field::Seed => "seed",
            Field::Px => "px",
            Field::Light => "light",
            Field::Edge => "edge",
            Field::EdgeNormal => "edge_normal",
            Field::S => "s",
            Field::D => "d",
            Field::Pressure => "pressure",
            Field::Angle => "angle",
            Field::Tilt => "tilt",
            Field::Velocity => "velocity",
            Field::Below => "below",
        }
    }

    pub fn parse(name: &str) -> Option<Field> {
        Field::ALL.iter().copied().find(|f| f.name() == name)
    }

    /// `None` for `light`, which has no type until #256 lands.
    pub fn ty(self) -> Option<Ty> {
        Some(match self {
            Field::Position | Field::Normal => Ty::Vec3,
            Field::Uv | Field::Size | Field::EdgeNormal => Ty::Vec2,
            Field::Paint | Field::Below => Ty::Color,
            Field::Light => return None,
            _ => Ty::Num,
        })
    }

    pub fn surface(self) -> Surface {
        match self {
            Field::Position
            | Field::Normal
            | Field::Uv
            | Field::Size
            | Field::Paint
            | Field::Seed
            | Field::Px
            | Field::Light => Surface::Any,
            Field::Below => Surface::Raster,
            _ => Surface::TwoD,
        }
    }
}

/// What a retired name of the SkSL `Mark` became, for the fix of E0360.
pub fn renamed(name: &str) -> Option<&'static str> {
    Some(match name {
        "p" => "m.position.xy",
        "len" => "m.size.x",
        "width" => "m.size.y",
        "u" => "m.s / m.size.x",
        "v" => "m.d / (m.size.y / 2)",
        _ => return None,
    })
}

/// Every non-noise builtin. Each keeps its `values` meaning; a backend lowers it to a guarded
/// form where the target's own function differs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Builtin {
    Abs,
    Floor,
    Round,
    Fract,
    Sqrt,
    Sin,
    Cos,
    Exp,
    Min,
    Max,
    Clamp,
    Lerp,
    Step,
    Smoothstep,
    Pow,
    Mod,
    Atan2,
    Rad,
    Dot,
    Length,
    Normalize,
    Rot,
    Cover,
}

impl Builtin {
    pub const ALL: &'static [Builtin] = &[
        Builtin::Abs,
        Builtin::Floor,
        Builtin::Round,
        Builtin::Fract,
        Builtin::Sqrt,
        Builtin::Sin,
        Builtin::Cos,
        Builtin::Exp,
        Builtin::Min,
        Builtin::Max,
        Builtin::Clamp,
        Builtin::Lerp,
        Builtin::Step,
        Builtin::Smoothstep,
        Builtin::Pow,
        Builtin::Mod,
        Builtin::Atan2,
        Builtin::Rad,
        Builtin::Dot,
        Builtin::Length,
        Builtin::Normalize,
        Builtin::Rot,
        Builtin::Cover,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Builtin::Abs => "abs",
            Builtin::Floor => "floor",
            Builtin::Round => "round",
            Builtin::Fract => "fract",
            Builtin::Sqrt => "sqrt",
            Builtin::Sin => "sin",
            Builtin::Cos => "cos",
            Builtin::Exp => "exp",
            Builtin::Min => "min",
            Builtin::Max => "max",
            Builtin::Clamp => "clamp",
            Builtin::Lerp => "lerp",
            Builtin::Step => "step",
            Builtin::Smoothstep => "smoothstep",
            Builtin::Pow => "pow",
            Builtin::Mod => "mod",
            Builtin::Atan2 => "atan2",
            Builtin::Rad => "rad",
            Builtin::Dot => "dot",
            Builtin::Length => "length",
            Builtin::Normalize => "normalize",
            Builtin::Rot => "rot",
            Builtin::Cover => "cover",
        }
    }

    pub fn parse(name: &str) -> Option<Builtin> {
        Builtin::ALL.iter().copied().find(|b| b.name() == name)
    }

    pub fn arity(self) -> usize {
        match self {
            Builtin::Abs
            | Builtin::Floor
            | Builtin::Round
            | Builtin::Fract
            | Builtin::Sqrt
            | Builtin::Sin
            | Builtin::Cos
            | Builtin::Exp
            | Builtin::Rad
            | Builtin::Length
            | Builtin::Normalize
            | Builtin::Cover => 1,
            Builtin::Min
            | Builtin::Max
            | Builtin::Step
            | Builtin::Pow
            | Builtin::Mod
            | Builtin::Atan2
            | Builtin::Dot
            | Builtin::Rot => 2,
            Builtin::Clamp | Builtin::Lerp | Builtin::Smoothstep => 3,
        }
    }
}

/// The noise functions. Each takes a vec2 point and seeds itself from the stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Noise {
    Hash,
    Hash2,
    Value,
    Simplex,
    Worley,
    Fbm,
}

impl Noise {
    pub const ALL: &'static [Noise] = &[Noise::Hash, Noise::Hash2, Noise::Value, Noise::Simplex, Noise::Worley, Noise::Fbm];

    pub fn name(self) -> &'static str {
        match self {
            Noise::Hash => "hash",
            Noise::Hash2 => "hash2",
            Noise::Value => "value",
            Noise::Simplex => "simplex",
            Noise::Worley => "worley",
            Noise::Fbm => "fbm",
        }
    }

    pub fn parse(name: &str) -> Option<Noise> {
        Noise::ALL.iter().copied().find(|n| n.name() == name)
    }

    pub fn ty(self) -> Ty {
        match self {
            Noise::Hash2 => Ty::Vec2,
            _ => Ty::Num,
        }
    }
}

/// A reduction over a list: `min`, `max`, or `sum` of a comprehension or a literal list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Reduce {
    Min,
    Max,
    Sum,
}

impl Reduce {
    pub fn name(self) -> &'static str {
        match self {
            Reduce::Min => "min",
            Reduce::Max => "max",
            Reduce::Sum => "sum",
        }
    }

    pub fn parse(name: &str) -> Option<Reduce> {
        match name {
            "min" => Some(Reduce::Min),
            "max" => Some(Reduce::Max),
            "sum" => Some(Reduce::Sum),
            _ => None,
        }
    }
}

/// The most items the comprehensions of one expression may multiply to.
pub const MAX_TRIPS: u32 = 256;

/// Every name a body resolves before the author's own: the builtins, the noise functions, the
/// reductions, and `m`. A docs test asserts the brushes topic names each one.
pub fn builtin_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = Builtin::ALL.iter().map(|b| b.name()).collect();
    names.extend(Noise::ALL.iter().map(|n| n.name()));
    names.push("sum");
    names.push("m");
    names
}

/// Why a parameter, `let`, helper, or helper parameter cannot take `name`, or `None` when it
/// can. Keywords are the parser's to reject, and backend words are prefixed away, so `half`
/// and `float` are free.
pub fn taken(name: &str) -> Option<&'static str> {
    if name == "m" {
        return Some("the sample");
    }
    if Field::parse(name).is_some() {
        return Some("a field of `m`");
    }
    if Builtin::parse(name).is_some() || Noise::parse(name).is_some() || Reduce::parse(name).is_some() {
        return Some("a builtin");
    }
    None
}

/// Names `values` has and a body does not, with what to do instead.
pub fn values_only(name: &str) -> Option<&'static str> {
    Some(match name {
        "rand" | "pick" => "a random value is seeded by the draw site; pass it in as a parameter, or use `hash`",
        "mix" => "a body has no color arithmetic; write `lerp(a.rgb b.rgb k)`",
        "polar" => "write `(r * cos(deg),r * sin(deg))`",
        "shade" | "lit" | "fade" | "line" | "oklch" | "ramp" | "grad" | "radial" | "conic" => {
            "a body has no color helpers; pass the color in as a parameter"
        }
        "perlin" | "ridged" => "a body's noise is `hash hash2 value simplex worley fbm` over a point",
        "concat" | "corners" | "cylinder" | "key" | "t" => "it has no meaning for one sample",
        _ => return None,
    })
}
