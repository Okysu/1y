//! Source positions and spans for AST nodes.
//!
//! Every AST node carries a [`Span`] so that later phases (parser errors,
//! runtime tracebacks) can point at the exact source range.

/// A single point in the source text.
///
/// `offset` is a 0-based byte offset into the source; `line` and `col` are
/// 1-based for human-friendly error messages (`col` counts Unicode scalars,
/// not bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pos {
    pub offset: usize,
    pub line: u32,
    pub col: u32,
}

impl Pos {
    pub const ZERO: Pos = Pos {
        offset: 0,
        line: 1,
        col: 1,
    };

    pub fn new(offset: usize, line: u32, col: u32) -> Self {
        Pos {
            offset,
            line,
            col,
        }
    }

    /// Advance the position by a single character. Handles newline tracking.
    pub fn advance(self, ch: char) -> Self {
        match ch {
            '\n' => Pos {
                offset: self.offset + ch.len_utf8(),
                line: self.line + 1,
                col: 1,
            },
            '\r' => Pos {
                // Ignore bare CR for column purposes; CRLF handled by the \n branch.
                offset: self.offset + ch.len_utf8(),
                line: self.line,
                col: self.col,
            },
            _ => Pos {
                offset: self.offset + ch.len_utf8(),
                line: self.line,
                col: self.col + 1,
            },
        }
    }
}

impl Default for Pos {
    fn default() -> Self {
        Pos::ZERO
    }
}

impl std::fmt::Display for Pos {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}, col {}", self.line, self.col)
    }
}

/// A half-open source range `[start, end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: Pos,
    pub end: Pos,
}

impl Span {
    pub fn new(start: Pos, end: Pos) -> Self {
        Span { start, end }
    }

    /// Span covering a single position (e.g. an EOF token).
    pub fn at(pos: Pos) -> Self {
        Span {
            start: pos,
            end: pos,
        }
    }

    pub fn dummy() -> Self {
        Span::at(Pos::ZERO)
    }

    /// Smallest span containing both `self` and `other`.
    pub fn union(self, other: Span) -> Span {
        let start = if self.start.offset < other.start.offset {
            self.start
        } else {
            other.start
        };
        let end = if self.end.offset > other.end.offset {
            self.end
        } else {
            other.end
        };
        Span { start, end }
    }
}

impl Default for Span {
    fn default() -> Self {
        Span::dummy()
    }
}

impl std::fmt::Display for Span {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.start.line == self.end.line {
            write!(f, "line {}, col {}-{}", self.start.line, self.start.col, self.end.col)
        } else {
            write!(
                f,
                "line {} col {} - line {} col {}",
                self.start.line, self.start.col, self.end.line, self.end.col
            )
        }
    }
}

/// Convenience trait: anything that knows its own [`Span`].
pub trait Spanned {
    fn span(&self) -> Span;
}
