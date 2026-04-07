use std::path::PathBuf;

/// All errors that `lort` can produce during parsing and processing.
///
/// Uses `thiserror` for ergonomic error definitions.
/// Application-level orchestration uses `anyhow` on top of these.
#[derive(Debug, thiserror::Error)]
pub enum LortError {
    /// Multi-name `import a, b` statement found.
    /// The user must run `ruff format` to split these first.
    #[error(
        "{file}:{line}: Multi-name import statement found (`{text}`).\n  \
         Run 'ruff format' to split multi-name imports."
    )]
    MultiNameImport {
        file: PathBuf,
        line: usize,
        text: String,
    },

    /// Backslash continuation in an import statement.
    /// The user must run `ruff format` to convert to parenthesized form.
    #[error(
        "{file}:{line}: Backslash continuation in import statement.\n  \
         Run 'ruff format' to convert to parenthesized form."
    )]
    BackslashContinuation { file: PathBuf, line: usize },

    /// `__future__` import mixed with non-`__future__` imports
    /// in the same contiguous block.
    #[error(
        "{file}:{line}: __future__ imports must appear in their own block \
         before other imports."
    )]
    FutureMixedWithOther { file: PathBuf, line: usize },

    /// Generic parse error for malformed import syntax.
    #[error("{file}:{line}: Failed to parse import: {reason}")]
    ParseError {
        file: PathBuf,
        line: usize,
        reason: String,
    },

    /// I/O error while reading or writing a file.
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}
