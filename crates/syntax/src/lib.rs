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

/// A source file and every syntax diagnostic in it. The file keeps each statement the parser could read.
pub struct Parsed {
    pub file: ast::File,
    pub diagnostics: Vec<Diagnostic>,
}

/// Parses one `.art` source, including a broken one. Diagnostics are empty when the file is clean.
pub fn parse_partial(src: &str) -> Parsed {
    parse::parse_partial(src)
}

/// Parses one `.art` source. Returns every syntax error found, not just the first.
/// A broken file is `Err`; `fmt` uses that to refuse to rewrite it.
pub fn parse(src: &str) -> Result<ast::File, Vec<Diagnostic>> {
    let parsed = parse_partial(src);
    if parsed.diagnostics.is_empty() {
        Ok(parsed.file)
    } else {
        Err(parsed.diagnostics)
    }
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
