use ramose_syntax::ast::Expr;

pub fn absolute_channel<'a>(key: &str, value: &'a Expr) -> Option<&'a Expr> {
    if !offset_channel(key) {
        return None;
    }
    match value {
        Expr::Call { func, args, named }
            if func == "abs" && args.len() == 1 && named.is_empty() =>
        {
            Some(&args[0])
        }
        _ => None,
    }
}

pub fn offset_channel(key: &str) -> bool {
    matches!(key, "at" | "rot" | "scale")
}
