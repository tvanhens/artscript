use serde::Serialize;
use std::fmt;

use crate::ast::Span;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

/// One message written for a model to act on: where, what, and the concrete fix.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Diagnostic {
    pub severity: Severity,
    /// `E0101` for parse errors, `E02xx` targets and modules, `E03xx` evaluation,
    /// `C01xx` checks, `L01xx` lints.
    pub code: String,
    /// Project-relative path of the source file; empty until the caller fills it in.
    pub file: String,
    pub span: Span,
    /// Node path such as `hero.body.head`, or empty.
    pub node: String,
    pub message: String,
    /// Extra lines shown under the message (measured values, available names).
    pub notes: Vec<String>,
    pub fix: Option<String>,
}

impl Diagnostic {
    pub fn error(code: &str, span: Span, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            code: code.into(),
            file: String::new(),
            span,
            node: String::new(),
            message: message.into(),
            notes: Vec::new(),
            fix: None,
        }
    }

    pub fn warning(code: &str, span: Span, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            ..Self::error(code, span, message)
        }
    }

    pub fn with_fix(mut self, fix: impl Into<String>) -> Self {
        self.fix = Some(fix.into());
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    pub fn at_node(mut self, node: impl Into<String>) -> Self {
        self.node = node.into();
        self
    }

    pub fn in_file(mut self, file: impl Into<String>) -> Self {
        self.file = file.into();
        self
    }
}

impl fmt::Display for Diagnostic {
    /// ```text
    /// error[E0311] art/chars/hero.art:18:7  hero.body.torso.tunic
    ///   tunic needs @torso, but the host `goblin` has no part named torso
    ///   parts available: chest, belly
    ///   fix: rename goblin's `chest` to `torso`
    /// ```
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sev = match self.severity {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
        };
        write!(f, "{sev}[{}] {}:{}", self.code, self.file, self.span)?;
        if !self.node.is_empty() {
            write!(f, "  {}", self.node)?;
        }
        write!(f, "\n  {}", self.message)?;
        for n in &self.notes {
            write!(f, "\n  {n}")?;
        }
        if let Some(fix) = &self.fix {
            write!(f, "\n  fix: {fix}")?;
        }
        Ok(())
    }
}
