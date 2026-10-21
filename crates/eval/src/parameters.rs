use std::collections::BTreeMap;

use ramose_syntax::ast::{Expr, SceneKind, UnOp};

pub type Parameters = BTreeMap<String, Expr>;

pub fn parse_parameter(text: &str) -> Result<(String, Expr), String> {
    let (name, value) = text.split_once('=').ok_or("--set needs name=value")?;
    if name.is_empty()
        || !name
            .chars()
            .enumerate()
            .all(|(i, c)| c == '_' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit()))
    {
        return Err(format!("invalid parameter name `{name}`"));
    }
    let parsed = ramose_syntax::parse(&format!("let value = {value}\n"))
        .map_err(|_| format!("invalid value for `{name}`; use a literal number, word, boolean, color, string, size, tuple, or list"))?;
    let Some(scene) = parsed.scenes().next() else {
        return Err(format!("missing value for `{name}`"));
    };
    let SceneKind::Let { value, .. } = &scene.kind else {
        return Err(format!("invalid value for `{name}`"));
    };
    if parsed.items.len() != 1 || !parsed.directives.is_empty() || !literal(value) {
        return Err(format!(
            "--set {name} accepts literal values, not expressions"
        ));
    }
    Ok((name.to_string(), value.clone()))
}

fn literal(value: &Expr) -> bool {
    match value {
        Expr::Num(n) | Expr::Percent(n) | Expr::Angle(n) | Expr::Time(n) => n.is_finite(),
        Expr::Hex(_) | Expr::Str(_) | Expr::Ident(_) => true,
        Expr::Size(a, b) => literal(a) && literal(b),
        Expr::Tuple(xs) | Expr::List(xs) => xs.iter().all(literal),
        Expr::Unary(UnOp::Neg, value) => {
            matches!(
                value.as_ref(),
                Expr::Num(_) | Expr::Percent(_) | Expr::Angle(_) | Expr::Time(_)
            ) && literal(value)
        }
        _ => false,
    }
}
