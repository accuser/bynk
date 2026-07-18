//! Source position spans.

/// A byte range in the source. Half-open: `[start, end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub fn range(&self) -> std::ops::Range<usize> {
        self.start..self.end
    }

    /// This span shifted right by `delta` bytes. Used to rebase spans produced
    /// against a substring (e.g. a re-lexed interpolation hole) into the full
    /// source. (#716.)
    pub fn offset(self, delta: usize) -> Span {
        Span {
            start: self.start + delta,
            end: self.end + delta,
        }
    }

    /// Span covering both `self` and `other` (the smallest enclosing range).
    pub fn merge(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

impl From<std::ops::Range<usize>> for Span {
    fn from(r: std::ops::Range<usize>) -> Self {
        Span {
            start: r.start,
            end: r.end,
        }
    }
}

/// 1-indexed (line, column) of a byte offset in `source`. Columns count
/// characters, not bytes. Lives in the syntax leaf so every layer that maps a
/// span to a position — the emitter's assertion locations, `bynkc`'s `short`
/// rendering, and (slice 6) `bynk-render` — shares one implementation.
pub fn line_col(source: &str, offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}
