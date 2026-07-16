//! Shared error types for the `1y` frontend (lexer + parser).
//!
//! Errors are *values*, never panics. Each error carries a [`Span`] and a
//! human-readable message; later phases can render them with a source
//! snippet via [`SourceError::render`].

use crate::ast::Span;

/// A single diagnostic.
#[derive(Debug, Clone)]
pub struct SourceError {
    pub span: Span,
    pub message: String,
    /// Suggested fix or hint (shown after the main message, optional).
    pub hint: Option<String>,
}

impl SourceError {
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        SourceError {
            span,
            message: message.into(),
            hint: None,
        }
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// The message plus optional hint, as a single string. Used when wrapping
    /// a `SourceError` into a runtime error so the hint is not lost.
    pub fn full_message(&self) -> String {
        match &self.hint {
            Some(h) => format!("{}\n  hint: {}", self.message, h),
            None => self.message.clone(),
        }
    }

    /// Render this error against a slice of source text. Produces a multi-line
    /// string with a `^^^` underline under the offending span.
    pub fn render(&self, source: &str) -> String {
        let span = self.span;
        let line_idx = (span.start.line as usize).saturating_sub(1);
        let line_str = source.lines().nth(line_idx).unwrap_or("");

        let mut out = String::new();
        out.push_str(&format!("error: {}\n", self.message));
        out.push_str(&format!("  --> {}\n", span));
        out.push_str(&format!("   | {}\n", line_str));

        // Underline.
        let start_col = span.start.col.max(1) as usize;
        let end_col = if span.start.line == span.end.line {
            span.end.col.max(1) as usize
        } else {
            line_str.chars().count() + 1
        };
        let pad = " ".repeat(start_col.saturating_sub(1));
        let carets = "^".repeat(end_col.saturating_sub(start_col).max(1));
        out.push_str(&format!("   | {}{}\n", pad, carets));
        if let Some(hint) = &self.hint {
            out.push_str(&format!("   = hint: {}\n", hint));
        }
        out
    }
}

impl std::fmt::Display for SourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "error at {}: {}", self.span, self.message)
    }
}

impl std::error::Error for SourceError {}

/// Result of lexing or parsing a whole file. The frontend tries to recover
/// from errors and keep going, so a failed run can produce both a partial
/// result and a list of errors.
#[derive(Debug, Default, Clone)]
pub struct ErrorReport {
    pub errors: Vec<SourceError>,
}

impl ErrorReport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, err: SourceError) {
        self.errors.push(err);
    }

    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn into_result<T>(self, ok: T) -> Result<T, Vec<SourceError>> {
        if self.errors.is_empty() {
            Ok(ok)
        } else {
            Err(self.errors)
        }
    }

    pub fn render_all(&self, source: &str) -> String {
        self.errors
            .iter()
            .map(|e| e.render(source))
            .collect::<Vec<_>>()
            .join("\n")
    }
}
