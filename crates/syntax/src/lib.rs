pub mod ast;
mod diag;
mod fmt;
mod lex;
mod parse;

pub use diag::{Diagnostic, Severity};

/// Words reserved in v0.1 even where unimplemented. Using one as a name is error E0105.
pub const RESERVED: &[&str] = &[
    "shader", "sksl", "field", "light", "brush", "warp", "backdrop", "profile", "pose", "joint",
    "ease", "t", "frames", "anim", "key",
];

/// Parses one `.art` source. Returns every syntax error found, not just the first.
pub fn parse(src: &str) -> Result<ast::File, Vec<Diagnostic>> {
    parse::parse_file(src)
}

/// Prints a file in the one canonical format. `format(parse(format(f))) == format(f)`.
pub fn format(file: &ast::File) -> String {
    fmt::format_file(file)
}

/// Parses `src` and prints it in canonical form, keeping comments attached by line.
/// `format_source` applied twice is the same string.
pub fn format_source(src: &str) -> Result<String, Vec<Diagnostic>> {
    let file = parse(src)?;
    let (comments, blanks) = fmt::source_notes(src);
    Ok(fmt::format_file_with_comments(&file, &comments, &blanks))
}
