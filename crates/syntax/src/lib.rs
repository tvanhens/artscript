pub mod ast;
mod diag;
mod fmt;
mod lex;
mod parse;
mod words;

pub use diag::{Diagnostic, Severity};

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
