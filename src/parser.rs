use std::path::Path;

use crate::error::LortError;

/// Whether the statement is `import X` or `from X import Y`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportKind {
    Import,
    FromImport,
}

/// A single imported name, optionally aliased.
///
/// For `from os.path import join as j`, this would be
/// `ImportedName { name: "join", alias: Some("j") }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedName {
    pub name: String,
    pub alias: Option<String>,
}

/// A parsed import statement with all metadata needed for sorting
/// and reconstruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportStatement {
    pub kind: ImportKind,
    /// The module path (e.g. `os.path`, `..bar`, `.`).
    pub module_path: String,
    /// Imported names for `from` imports. Empty for plain `import`.
    pub names: Vec<ImportedName>,
    /// Alias for plain `import X as Y` statements.
    pub alias: Option<String>,
    /// Inline comment on the same line (e.g. `# noqa`).
    pub inline_comment: Option<String>,
    /// 1-based line number in the source file.
    pub line_number: usize,
    /// Whether this is a relative import (starts with `.`).
    pub is_relative: bool,
    /// Whether this is a `__future__` import.
    pub is_future: bool,
    /// Whether the original used multi-line parenthesized form.
    pub is_multiline: bool,
}

/// A segment of a Python source file: either an import statement
/// or a non-import line (code, comments, blank lines).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileSegment {
    Import(ImportStatement),
    NonImport(String),
}

/// Parse a Python source file into a sequence of [`FileSegment`]s.
///
/// Only parses top-level import statements. Imports inside `if` guards
/// (`if TYPE_CHECKING:`, `try/except`, etc.) are treated as non-import lines,
/// as are lines that start inside a triple-quoted string (e.g. a docstring).
///
/// # Arguments
///
/// * `source` - The full source text of the Python file.
/// * `file_path` - Path for error messages.
///
/// # Errors
///
/// Returns [`LortError`] for:
/// - Multi-name `import a, b` statements
/// - Backslash continuations in imports
pub fn parse_file(source: &str, file_path: &Path) -> Result<Vec<FileSegment>, LortError> {
    let lines: Vec<&str> = source.lines().collect();
    let mut segments = Vec::with_capacity(lines.len());
    let mut i = 0;
    // Quote byte of the triple-quoted string open at the start of the current line.
    let mut open_string: Option<u8> = None;
    // Track indentation depth to skip guarded imports.
    // We only parse imports at column 0 (no leading whitespace).
    // NOTE for Rust newcomers: `while` with manual index is used here
    // instead of `for` because multi-line imports consume multiple lines.
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();
        let line_number = i + 1; // 1-based
        let starts_in_string = open_string.is_some();
        // Real import lines hold no quotes outside comments, so scanning them is a no-op.
        open_string = open_triple_quote_after(line, open_string);

        // Lines inside a triple-quoted string, empty lines and comment-only
        // lines are non-import.
        if starts_in_string || trimmed.is_empty() || trimmed.starts_with('#') {
            segments.push(FileSegment::NonImport(line.to_string()));
            i += 1;
            continue;
        }

        // Only parse imports at the top level (no indentation).
        if line.starts_with(' ') || line.starts_with('\t') {
            segments.push(FileSegment::NonImport(line.to_string()));
            i += 1;
            continue;
        }

        // Check for backslash continuation on import lines.
        if (trimmed.starts_with("import ") || trimmed.starts_with("from "))
            && trimmed.ends_with('\\')
        {
            return Err(LortError::BackslashContinuation {
                file: file_path.to_path_buf(),
                line: line_number,
            });
        }

        // Try to parse `import X` statement.
        if let Some(rest) = trimmed.strip_prefix("import ") {
            let stmt = parse_import_statement(rest, line_number, file_path)?;
            segments.push(FileSegment::Import(stmt));
            i += 1;
            continue;
        }

        // Try to parse `from X import Y` statement (possibly multi-line).
        if trimmed.starts_with("from ") {
            let (stmt, lines_consumed) = parse_from_import(&lines, i, file_path)?;
            segments.push(FileSegment::Import(stmt));
            i += lines_consumed;
            continue;
        }

        // Everything else is non-import code.
        segments.push(FileSegment::NonImport(line.to_string()));
        i += 1;
    }

    // Preserve trailing newline if the original source had one.
    if source.ends_with('\n') && !segments.is_empty() {
        // lines() strips the trailing newline, so we add an empty
        // segment to reconstruct it during output.
    }

    Ok(segments)
}

/// Find the triple-quoted string still open at the end of `line`.
///
/// Scans left to right, skipping comments and single-line strings (honoring `\`
/// escapes), so a `"""` inside `'...'` or after `#` opens nothing. An open
/// triple-quoted string closes only on its own unescaped delimiter.
///
/// Works on bytes: in UTF-8, the ASCII bytes matched here never occur inside a
/// multibyte character. String prefixes need no handling: even in a raw string, a
/// `\` keeps the quote after it from closing the string (the `\` is kept in the value).
///
/// # Arguments
///
/// * `line` - One source line, without its newline.
/// * `open` - Quote byte (`"` or `'`) of the triple-quoted string open at the
///   start of the line, or `None` when the line starts in code.
///
/// # Returns
///
/// The quote byte of the triple-quoted string open at the end of the line, if any.
fn open_triple_quote_after(line: &str, open: Option<u8>) -> Option<u8> {
    let bytes = line.as_bytes();
    let mut open = open;
    let mut i = 0;
    while i < bytes.len() {
        let byte = bytes[i];
        match open {
            Some(_) if byte == b'\\' => i += 2,
            // `[quote; 3]` is an array of three copies, i.e. the delimiter `"""` or `'''`.
            Some(quote) if bytes[i..].starts_with(&[quote; 3]) => {
                open = None;
                i += 3;
            }
            Some(_) => i += 1,
            None if byte == b'#' => break,
            None if (byte == b'"' || byte == b'\'') && bytes[i..].starts_with(&[byte; 3]) => {
                open = Some(byte);
                i += 3;
            }
            None if byte == b'"' || byte == b'\'' => i = skip_short_string(bytes, i + 1, byte),
            None => i += 1,
        }
    }
    open
}

/// Return the index just past the closing `quote` of a single-line string.
///
/// `start` is the index right after the opening quote. Returns `bytes.len()` when
/// the string is not closed on this line.
fn skip_short_string(bytes: &[u8], start: usize, quote: u8) -> usize {
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            byte if byte == quote => return i + 1,
            _ => i += 1,
        }
    }
    bytes.len()
}

/// Parse a plain `import X` or `import X as Y` statement.
///
/// The `rest` parameter is everything after `"import "`.
/// Rejects multi-name imports like `import a, b`.
fn parse_import_statement(
    rest: &str,
    line_number: usize,
    file_path: &Path,
) -> Result<ImportStatement, LortError> {
    // Split off any inline comment.
    let (code, inline_comment) = split_inline_comment(rest);
    let code = code.trim();

    // Reject multi-name imports: `import a, b`.
    if code.contains(',') {
        return Err(LortError::MultiNameImport {
            file: file_path.to_path_buf(),
            line: line_number,
            text: format!("import {code}"),
        });
    }

    // Parse optional alias: `import X as Y`.
    let (module_path, alias) = parse_alias(code);

    Ok(ImportStatement {
        kind: ImportKind::Import,
        module_path: module_path.to_string(),
        names: Vec::new(),
        alias: alias.map(String::from),
        inline_comment: inline_comment.map(String::from),
        line_number,
        is_relative: false, // plain `import` can't be relative
        is_future: module_path == "__future__",
        is_multiline: false,
    })
}

/// Parse a `from X import Y` statement, handling multi-line parenthesized form.
///
/// Returns the parsed statement and the number of source lines consumed.
fn parse_from_import(
    lines: &[&str],
    start: usize,
    file_path: &Path,
) -> Result<(ImportStatement, usize), LortError> {
    let line_number = start + 1; // 1-based
    let first_line = lines[start].trim();

    // Extract module path: everything between "from " and " import".
    let after_from = first_line
        .strip_prefix("from ")
        .expect("caller verified 'from ' prefix");
    let import_pos = after_from.find(" import ");
    let import_pos = match import_pos {
        Some(p) => p,
        None => {
            return Err(LortError::ParseError {
                file: file_path.to_path_buf(),
                line: line_number,
                reason: "expected 'import' keyword after module path".to_string(),
            });
        }
    };

    let module_path = after_from[..import_pos].trim();
    let is_relative = module_path.starts_with('.');
    let is_future = module_path == "__future__";
    let names_part = after_from[import_pos + 8..].trim(); // skip " import "

    // Check for backslash continuation.
    if names_part.ends_with('\\') {
        return Err(LortError::BackslashContinuation {
            file: file_path.to_path_buf(),
            line: line_number,
        });
    }

    // Multi-line parenthesized import: `from X import (\n...\n)`
    if let Some(after_paren) = names_part.strip_prefix('(') {
        let mut collected = String::new();
        let mut inline_comment = None;

        // Handle case where opening paren and some names are on the same line.
        if let Some(close) = after_paren.find(')') {
            // Single-line parenthesized: `from X import (a, b)`
            let (code, comment) = split_inline_comment(&after_paren[..close]);
            collected.push_str(code.trim());
            inline_comment = comment.map(String::from);
        } else {
            // True multi-line: consume until closing paren.
            collected.push_str(after_paren.trim());
            let mut j = start + 1;
            while j < lines.len() {
                let l = lines[j].trim();
                if let Some(close) = l.find(')') {
                    let before_close = &l[..close];
                    let (code, comment) = split_inline_comment(before_close);
                    if !collected.is_empty() && !code.trim().is_empty() {
                        collected.push_str(", ");
                    }
                    collected.push_str(code.trim());
                    // Check for inline comment after the closing paren.
                    let after_close = l[close + 1..].trim();
                    if let Some(c) = extract_comment(after_close) {
                        inline_comment = Some(c.to_string());
                    } else if comment.is_some() {
                        inline_comment = comment.map(String::from);
                    }
                    j += 1;
                    break;
                }
                // Check for backslash in multi-line import.
                if l.ends_with('\\') {
                    return Err(LortError::BackslashContinuation {
                        file: file_path.to_path_buf(),
                        line: j + 1,
                    });
                }
                let (code, _comment) = split_inline_comment(l);
                let code = code.trim();
                if !code.is_empty() {
                    if !collected.is_empty() {
                        collected.push_str(", ");
                    }
                    collected.push_str(code);
                }
                j += 1;
            }
            let names = parse_name_list(&collected);
            return Ok((
                ImportStatement {
                    kind: ImportKind::FromImport,
                    module_path: module_path.to_string(),
                    names,
                    alias: None,
                    inline_comment,
                    line_number,
                    is_relative,
                    is_future,
                    is_multiline: true,
                },
                j - start,
            ));
        }

        let names = parse_name_list(&collected);
        return Ok((
            ImportStatement {
                kind: ImportKind::FromImport,
                module_path: module_path.to_string(),
                names,
                alias: None,
                inline_comment,
                line_number,
                is_relative,
                is_future,
                is_multiline: false,
            },
            1,
        ));
    }

    // Single-line: `from X import a, b as c`
    let (code, inline_comment) = split_inline_comment(names_part);
    let names = parse_name_list(code.trim());

    Ok((
        ImportStatement {
            kind: ImportKind::FromImport,
            module_path: module_path.to_string(),
            names,
            alias: None,
            inline_comment: inline_comment.map(String::from),
            line_number,
            is_relative,
            is_future,
            is_multiline: false,
        },
        1,
    ))
}

/// Parse a comma-separated list of imported names, each optionally aliased.
///
/// Handles: `a`, `a as b`, `a, b as c, d`.
fn parse_name_list(s: &str) -> Vec<ImportedName> {
    s.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|name_str| {
            let (name, alias) = parse_alias(name_str);
            ImportedName {
                name: name.to_string(),
                alias: alias.map(String::from),
            }
        })
        .collect()
}

/// Split `"X as Y"` into `("X", Some("Y"))`, or `"X"` into `("X", None)`.
fn parse_alias(s: &str) -> (&str, Option<&str>) {
    // Look for " as " to avoid matching substrings like "class".
    if let Some(pos) = s.find(" as ") {
        let name = s[..pos].trim();
        let alias = s[pos + 4..].trim();
        (name, Some(alias))
    } else {
        (s.trim(), None)
    }
}

/// Split a code fragment at the first `#` that indicates a comment.
///
/// Returns `(code_part, optional_comment_text)`.
/// The comment text includes the `#` prefix.
fn split_inline_comment(s: &str) -> (&str, Option<&str>) {
    // Naive approach: first `#` that isn't inside a string.
    // For import statements, names don't contain `#`, so this is safe.
    if let Some(pos) = s.find('#') {
        let code = s[..pos].trim_end();
        let comment = s[pos..].trim();
        (code, Some(comment))
    } else {
        (s, None)
    }
}

/// Extract a comment from a string that might be just a comment.
fn extract_comment(s: &str) -> Option<&str> {
    if s.starts_with('#') { Some(s) } else { None }
}

/// Reconstruct an [`ImportStatement`] back into source code.
///
/// Preserves multi-line parenthesized format when the original used it.
/// Single-line imports remain single-line.
///
/// # Arguments
///
/// * `stmt` - The import statement to format.
///
/// # Returns
///
/// One or more lines of Python source code (without trailing newline).
pub fn reconstruct_import(stmt: &ImportStatement) -> String {
    let mut out = String::with_capacity(80);

    match stmt.kind {
        ImportKind::Import => {
            out.push_str("import ");
            out.push_str(&stmt.module_path);
            // Handle alias for `import X as Y`.
            if let Some(ref alias) = stmt.alias {
                out.push_str(" as ");
                out.push_str(alias);
            }
        }
        ImportKind::FromImport => {
            out.push_str("from ");
            out.push_str(&stmt.module_path);
            out.push_str(" import ");

            // Format each name, handling optional aliases.
            let names: Vec<String> = stmt
                .names
                .iter()
                .map(|n| match &n.alias {
                    Some(a) => format!("{} as {}", n.name, a),
                    None => n.name.clone(),
                })
                .collect();

            if stmt.is_multiline {
                // Reconstruct parenthesized multi-line form with
                // 4-space indentation per PEP 8 / black / ruff defaults.
                out.push('(');
                for name in &names {
                    out.push_str("\n    ");
                    out.push_str(name);
                    out.push(',');
                }
                out.push_str("\n)");
            } else {
                out.push_str(&names.join(", "));
            }
        }
    }

    if let Some(ref comment) = stmt.inline_comment {
        out.push_str("  ");
        out.push_str(comment);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_path() -> PathBuf {
        PathBuf::from("test.py")
    }

    #[test]
    fn test_parse_simple_import() {
        let source = "import os\n";
        let segments = parse_file(source, &test_path()).unwrap();
        assert_eq!(segments.len(), 1);
        match &segments[0] {
            FileSegment::Import(stmt) => {
                assert_eq!(stmt.kind, ImportKind::Import);
                assert_eq!(stmt.module_path, "os");
                assert!(!stmt.is_relative);
                assert!(!stmt.is_future);
            }
            _ => panic!("expected Import segment"),
        }
    }

    #[test]
    fn test_parse_from_import() {
        let source = "from os.path import join, exists\n";
        let segments = parse_file(source, &test_path()).unwrap();
        assert_eq!(segments.len(), 1);
        match &segments[0] {
            FileSegment::Import(stmt) => {
                assert_eq!(stmt.kind, ImportKind::FromImport);
                assert_eq!(stmt.module_path, "os.path");
                assert_eq!(stmt.names.len(), 2);
                assert_eq!(stmt.names[0].name, "join");
                assert_eq!(stmt.names[1].name, "exists");
            }
            _ => panic!("expected Import segment"),
        }
    }

    #[test]
    fn test_parse_from_import_with_alias() {
        let source = "from os.path import join as j\n";
        let segments = parse_file(source, &test_path()).unwrap();
        match &segments[0] {
            FileSegment::Import(stmt) => {
                assert_eq!(stmt.names[0].name, "join");
                assert_eq!(stmt.names[0].alias.as_deref(), Some("j"));
            }
            _ => panic!("expected Import segment"),
        }
    }

    #[test]
    fn test_parse_multiline_parenthesized() {
        let source = "from typing import (\n    Any,\n    Dict,\n    List,\n)\n";
        let segments = parse_file(source, &test_path()).unwrap();
        assert_eq!(segments.len(), 1);
        match &segments[0] {
            FileSegment::Import(stmt) => {
                assert_eq!(stmt.module_path, "typing");
                assert_eq!(stmt.names.len(), 3);
                assert_eq!(stmt.names[0].name, "Any");
                assert_eq!(stmt.names[1].name, "Dict");
                assert_eq!(stmt.names[2].name, "List");
            }
            _ => panic!("expected Import segment"),
        }
    }

    #[test]
    fn test_reject_multi_name_import() {
        let source = "import os, sys\n";
        let result = parse_file(source, &test_path());
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Multi-name import"), "got: {err}");
    }

    #[test]
    fn test_reject_backslash_continuation() {
        let source = "from os.path import \\\n    join\n";
        let result = parse_file(source, &test_path());
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("Backslash continuation"),
            "got: {err}"
        );
    }

    #[test]
    fn test_relative_import() {
        let source = "from . import foo\n";
        let segments = parse_file(source, &test_path()).unwrap();
        match &segments[0] {
            FileSegment::Import(stmt) => {
                assert!(stmt.is_relative);
                assert_eq!(stmt.module_path, ".");
            }
            _ => panic!("expected Import segment"),
        }
    }

    #[test]
    fn test_future_import() {
        let source = "from __future__ import annotations\n";
        let segments = parse_file(source, &test_path()).unwrap();
        match &segments[0] {
            FileSegment::Import(stmt) => {
                assert!(stmt.is_future);
                assert_eq!(stmt.module_path, "__future__");
            }
            _ => panic!("expected Import segment"),
        }
    }

    #[test]
    fn test_inline_comment_preserved() {
        let source = "import os  # needed\n";
        let segments = parse_file(source, &test_path()).unwrap();
        match &segments[0] {
            FileSegment::Import(stmt) => {
                assert_eq!(stmt.inline_comment.as_deref(), Some("# needed"));
            }
            _ => panic!("expected Import segment"),
        }
    }

    #[test]
    fn test_indented_imports_are_non_import() {
        let source = "if True:\n    import os\n";
        let segments = parse_file(source, &test_path()).unwrap();
        assert!(matches!(segments[0], FileSegment::NonImport(_)));
        assert!(matches!(segments[1], FileSegment::NonImport(_)));
    }

    #[test]
    fn test_open_triple_quote_after() {
        let cases: [(&str, Option<u8>, Option<u8>); 13] = [
            ("x = 1", None, None),
            ("\"\"\"Doc.", None, Some(b'"')),
            ("X = r'''", None, Some(b'\'')),
            ("\"\"\"Doc.\"\"\"", None, None),
            ("\"\"\"\"\"\"", None, None),
            ("x = ''", None, None),
            ("# a \"\"\" here", None, None),
            ("X = '\"\"\"'", None, None),
            ("X = 'it\\'s' + \"\"\"", None, Some(b'"')),
            ("Says ''' here.", Some(b'"'), Some(b'"')),
            ("Has \\\"\"\" inside.", Some(b'"'), Some(b'"')),
            ("X = r\"\"\"a\\\"\"\"", None, Some(b'"')),
            ("end\"\"\" # \"\"\"", Some(b'"'), None),
        ];
        for (line, open, expected) in cases {
            assert_eq!(
                open_triple_quote_after(line, open),
                expected,
                "line: {line}"
            );
        }
    }

    #[test]
    fn test_import_lines_in_docstring_are_non_import() {
        let source = "\"\"\"Doc.\nimport os\nfrom a import b\n\"\"\"\nimport sys\n";
        let segments = parse_file(source, &test_path()).unwrap();
        assert!(
            segments[..4]
                .iter()
                .all(|s| matches!(s, FileSegment::NonImport(_)))
        );
        assert!(matches!(segments[4], FileSegment::Import(_)));
    }

    #[test]
    fn test_reconstruct_simple_import() {
        let stmt = ImportStatement {
            kind: ImportKind::Import,
            module_path: "os".to_string(),
            names: Vec::new(),
            alias: None,
            inline_comment: None,
            line_number: 1,
            is_relative: false,
            is_future: false,
            is_multiline: false,
        };
        assert_eq!(reconstruct_import(&stmt), "import os");
    }

    #[test]
    fn test_reconstruct_from_import_with_comment() {
        let stmt = ImportStatement {
            kind: ImportKind::FromImport,
            module_path: "os.path".to_string(),
            names: vec![
                ImportedName {
                    name: "join".to_string(),
                    alias: Some("j".to_string()),
                },
                ImportedName {
                    name: "exists".to_string(),
                    alias: None,
                },
            ],
            alias: None,
            inline_comment: Some("# utils".to_string()),
            line_number: 1,
            is_relative: false,
            is_future: false,
            is_multiline: false,
        };
        assert_eq!(
            reconstruct_import(&stmt),
            "from os.path import join as j, exists  # utils"
        );
    }

    #[test]
    fn test_reconstruct_multiline_import() {
        let stmt = ImportStatement {
            kind: ImportKind::FromImport,
            module_path: "sync_tools.sync".to_string(),
            names: vec![
                ImportedName {
                    name: "build_copy_plan".to_string(),
                    alias: None,
                },
                ImportedName {
                    name: "display_sync_plan".to_string(),
                    alias: None,
                },
                ImportedName {
                    name: "execute_sync_plan".to_string(),
                    alias: None,
                },
            ],
            alias: None,
            inline_comment: None,
            line_number: 1,
            is_relative: false,
            is_future: false,
            is_multiline: true,
        };
        assert_eq!(
            reconstruct_import(&stmt),
            "from sync_tools.sync import (\n\
             \x20   build_copy_plan,\n\
             \x20   display_sync_plan,\n\
             \x20   execute_sync_plan,\n\
             )"
        );
    }

    #[test]
    fn test_roundtrip_multiline_parenthesized() {
        let source = "from typing import (\n    Any,\n    Dict,\n    List,\n)\n";
        let segments = parse_file(source, &test_path()).unwrap();
        match &segments[0] {
            FileSegment::Import(stmt) => {
                assert!(stmt.is_multiline);
                let reconstructed = reconstruct_import(stmt);
                assert_eq!(
                    reconstructed,
                    "from typing import (\n    Any,\n    Dict,\n    List,\n)"
                );
            }
            _ => panic!("expected Import segment"),
        }
    }
}
