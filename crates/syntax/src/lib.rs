pub mod ast;
mod diag;
mod fmt;
mod lex;
mod parse;

pub use diag::{Diagnostic, Severity};

/// Words reserved in v0.1 even where unimplemented. Using one as a name is error E0105.
pub const RESERVED: &[&str] = &[
    "shader", "sksl", "field", "light", "brush", "warp", "backdrop", "profile", "pose", "joint", "ease", "t",
    "frames", "anim", "key",
];

/// Parses one `.art` source. Returns every syntax error found, not just the first.
pub fn parse(src: &str) -> Result<ast::File, Vec<Diagnostic>> {
    parse::parse_file(src)
}

/// Prints a file in the one canonical format. `format(parse(format(f))) == format(f)`.
pub fn format(file: &ast::File) -> String {
    fmt::format_file(file)
}
