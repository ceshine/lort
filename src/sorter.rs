use std::cmp::Ordering;
use std::path::Path;

use crate::error::LortError;
use crate::parser::{FileSegment, ImportKind, ImportStatement, reconstruct_import};
use crate::stdlib::typing_priority;

/// Compare two module path strings using length-first, segment-by-segment
/// ordering.
///
/// # Algorithm
///
/// For each pair of segments at the same depth:
/// 1. Compare by length — shorter segment wins.
/// 2. If lengths are equal, compare alphabetically.
/// 3. If one path is a prefix of the other, the shorter path wins
///    (parent before child).
///
/// # Examples
///
/// ```
/// # use lort::sorter::compare_module_path;
/// # use std::cmp::Ordering;
/// assert_eq!(compare_module_path("os", "abc"), Ordering::Less);       // 2 < 3
/// assert_eq!(compare_module_path("a.b", "a.b.c"), Ordering::Less);   // parent first
/// assert_eq!(compare_module_path("a.bc", "a.b"), Ordering::Greater); // bc(2) > b(1)
/// ```
pub fn compare_module_path(a: &str, b: &str) -> Ordering {
    // NOTE for Rust newcomers: `split('.')` returns a lazy iterator.
    // We zip the two iterators to compare segment-by-segment without
    // allocating a Vec. The `zip` stops at the shorter iterator.
    let mut a_segs = a.split('.');
    let mut b_segs = b.split('.');
    let mut a_count = 0usize;
    let mut b_count = 0usize;

    loop {
        match (a_segs.next(), b_segs.next()) {
            (Some(sa), Some(sb)) => {
                a_count += 1;
                b_count += 1;
                let by_len = sa.len().cmp(&sb.len());
                if by_len != Ordering::Equal {
                    return by_len;
                }
                let by_alpha = sa.cmp(sb);
                if by_alpha != Ordering::Equal {
                    return by_alpha;
                }
            }
            (Some(_), None) => {
                // `a` has more segments — `b` is a prefix (parent).
                return Ordering::Greater;
            }
            (None, Some(_)) => {
                return Ordering::Less;
            }
            (None, None) => {
                // Same number of segments and all equal.
                return a_count.cmp(&b_count);
            }
        }
    }
}

/// Compare two imported names using the same length-first rule.
///
/// The sort key uses the **original name**, not the alias.
/// Star import (`*`) has length 1.
fn compare_imported_name(a: &str, b: &str) -> Ordering {
    let by_len = a.len().cmp(&b.len());
    if by_len != Ordering::Equal {
        return by_len;
    }
    a.cmp(b)
}

/// Composite sort key for an import statement within a block.
///
/// The ordering is:
/// 1. `__future__` imports first (preserved at top).
/// 2. Non-typing before typing (typing pinned to bottom).
/// 3. `import` before `from`.
/// 4. Relative `from` before absolute `from`.
/// 5. Module path by length-first segment comparison.
fn import_sort_key(stmt: &ImportStatement) -> impl Ord {
    let is_future = !stmt.is_future; // false < true, so `!` puts future first
    // typing_priority: 0 = normal, 1 = typing-adjacent (collections.abc),
    // 2 = typing itself. Higher values sort later (bottom of block).
    let typing_pri = typing_priority(&stmt.module_path);
    let is_from = stmt.kind == ImportKind::FromImport;
    // Relative imports come before absolute within the `from` group.
    // For plain `import` (never relative), this is always false.
    let is_absolute = !stmt.is_relative;

    (
        is_future,
        typing_pri,
        is_from,
        is_absolute,
        PathSortKey(stmt.module_path.clone()),
    )
}

/// Wrapper type that implements [`Ord`] using [`compare_module_path`].
///
/// This lets us use the custom comparator inside tuple sort keys
/// via Rust's derived lexicographic ordering.
// NOTE for Rust newcomers: Rust requires types in sort keys to implement
// the `Ord` trait. We can't use a closure directly, so we wrap `String`
// and implement `Ord` manually.
#[derive(Debug, Clone, Eq, PartialEq)]
struct PathSortKey(String);

impl Ord for PathSortKey {
    fn cmp(&self, other: &Self) -> Ordering {
        compare_module_path(&self.0, &other.0)
    }
}

impl PartialOrd for PathSortKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Sort the imported names within a `from` import by length then alpha.
///
/// Modifies the statement in place.
fn sort_imported_names(stmt: &mut ImportStatement) {
    stmt.names
        .sort_by(|a, b| compare_imported_name(&a.name, &b.name));
}

/// Sort a contiguous block of import statements according to the spec.
///
/// Also sorts imported names within each `from` statement.
///
/// # Arguments
///
/// * `stmts` - Mutable slice of import statements forming one contiguous block.
fn sort_import_block(stmts: &mut [ImportStatement]) {
    // First, sort imported names within each `from` statement.
    for stmt in stmts.iter_mut() {
        if stmt.kind == ImportKind::FromImport {
            sort_imported_names(stmt);
        }
    }
    // Then sort the statements themselves.
    stmts.sort_by(|a, b| import_sort_key(a).cmp(&import_sort_key(b)));
}

/// Validate that `__future__` imports are not mixed with other imports
/// in the same contiguous block.
fn validate_future_imports(block: &[ImportStatement], file_path: &Path) -> Result<(), LortError> {
    let has_future = block.iter().any(|s| s.is_future);
    let has_non_future = block.iter().any(|s| !s.is_future);

    if has_future && has_non_future {
        // Report the line of the first __future__ import.
        let future_line = block
            .iter()
            .find(|s| s.is_future)
            .expect("has_future is true")
            .line_number;
        return Err(LortError::FutureMixedWithOther {
            file: file_path.to_path_buf(),
            line: future_line,
        });
    }
    Ok(())
}

/// Process a parsed file: sort all import blocks and reconstruct source.
///
/// # Arguments
///
/// * `segments` - The parsed file segments from [`crate::parser::parse_file`].
/// * `file_path` - Path for error messages.
///
/// # Returns
///
/// The reconstructed source text with sorted imports.
///
/// # Errors
///
/// Returns [`LortError::FutureMixedWithOther`] if `__future__` imports
/// are mixed with other imports in the same block.
pub fn sort_and_reconstruct(
    segments: Vec<FileSegment>,
    file_path: &Path,
) -> Result<String, LortError> {
    // Identify contiguous blocks of imports and sort each one.
    let mut result_lines: Vec<String> = Vec::with_capacity(segments.len());
    let mut current_block: Vec<ImportStatement> = Vec::new();

    for segment in segments {
        match segment {
            FileSegment::Import(stmt) => {
                current_block.push(stmt);
            }
            FileSegment::NonImport(line) => {
                if !current_block.is_empty() {
                    validate_future_imports(&current_block, file_path)?;
                    sort_import_block(&mut current_block);
                    for stmt in current_block.drain(..) {
                        result_lines.push(reconstruct_import(&stmt));
                    }
                }
                result_lines.push(line);
            }
        }
    }

    // Flush any remaining block at end of file.
    if !current_block.is_empty() {
        validate_future_imports(&current_block, file_path)?;
        sort_import_block(&mut current_block);
        for stmt in current_block.drain(..) {
            result_lines.push(reconstruct_import(&stmt));
        }
    }

    // Empty file — return as-is without adding a trailing newline.
    if result_lines.is_empty() {
        return Ok(String::new());
    }

    let mut output = result_lines.join("\n");
    // Ensure file ends with a newline.
    if !output.ends_with('\n') {
        output.push('\n');
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ImportedName;

    #[test]
    fn test_compare_module_path_length_first() {
        assert_eq!(compare_module_path("os", "abc"), Ordering::Less);
        assert_eq!(compare_module_path("os", "collections"), Ordering::Less);
        assert_eq!(compare_module_path("re", "csv"), Ordering::Less);
    }

    #[test]
    fn test_compare_module_path_alpha_tiebreak() {
        assert_eq!(compare_module_path("abc", "ast"), Ordering::Less);
        assert_eq!(compare_module_path("csv", "sys"), Ordering::Less);
    }

    #[test]
    fn test_compare_module_path_parent_before_child() {
        assert_eq!(compare_module_path("a.b", "a.b.c"), Ordering::Less);
        assert_eq!(compare_module_path("a.b.c", "a.b"), Ordering::Greater);
    }

    #[test]
    fn test_compare_module_path_segment_by_segment() {
        // xml.sax (seg2: 3) < xml.parsers (seg2: 7)
        assert_eq!(
            compare_module_path("xml.sax.handler", "xml.parsers.expat"),
            Ordering::Less
        );
    }

    #[test]
    fn test_compare_module_path_equal() {
        assert_eq!(compare_module_path("os.path", "os.path"), Ordering::Equal);
    }

    #[test]
    fn test_sort_imported_names() {
        let mut stmt = ImportStatement {
            kind: ImportKind::FromImport,
            module_path: "typing".to_string(),
            names: vec![
                ImportedName {
                    name: "Optional".to_string(),
                    alias: None,
                },
                ImportedName {
                    name: "Any".to_string(),
                    alias: None,
                },
                ImportedName {
                    name: "List".to_string(),
                    alias: None,
                },
                ImportedName {
                    name: "Dict".to_string(),
                    alias: None,
                },
            ],
            inline_comment: None,
            line_number: 1,
            is_relative: false,
            is_future: false,
        };
        sort_imported_names(&mut stmt);
        let names: Vec<&str> = stmt.names.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["Any", "Dict", "List", "Optional"]);
    }

    #[test]
    fn test_sort_block_import_before_from() {
        let mut block = vec![
            ImportStatement {
                kind: ImportKind::FromImport,
                module_path: "os.path".to_string(),
                names: vec![ImportedName {
                    name: "join".to_string(),
                    alias: None,
                }],
                inline_comment: None,
                line_number: 1,
                is_relative: false,
                is_future: false,
            },
            ImportStatement {
                kind: ImportKind::Import,
                module_path: "os".to_string(),
                names: Vec::new(),
                inline_comment: None,
                line_number: 2,
                is_relative: false,
                is_future: false,
            },
        ];
        sort_import_block(&mut block);
        assert_eq!(block[0].kind, ImportKind::Import);
        assert_eq!(block[1].kind, ImportKind::FromImport);
    }

    #[test]
    fn test_sort_block_typing_at_bottom() {
        let mut block = vec![
            ImportStatement {
                kind: ImportKind::FromImport,
                module_path: "typing".to_string(),
                names: vec![ImportedName {
                    name: "Any".to_string(),
                    alias: None,
                }],
                inline_comment: None,
                line_number: 1,
                is_relative: false,
                is_future: false,
            },
            ImportStatement {
                kind: ImportKind::Import,
                module_path: "os".to_string(),
                names: Vec::new(),
                inline_comment: None,
                line_number: 2,
                is_relative: false,
                is_future: false,
            },
            ImportStatement {
                kind: ImportKind::FromImport,
                module_path: "collections".to_string(),
                names: vec![ImportedName {
                    name: "defaultdict".to_string(),
                    alias: None,
                }],
                inline_comment: None,
                line_number: 3,
                is_relative: false,
                is_future: false,
            },
        ];
        sort_import_block(&mut block);
        assert_eq!(block[0].module_path, "os");
        assert_eq!(block[1].module_path, "collections");
        assert_eq!(block[2].module_path, "typing");
    }

    #[test]
    fn test_future_mixed_with_other_is_error() {
        let block = vec![
            ImportStatement {
                kind: ImportKind::FromImport,
                module_path: "__future__".to_string(),
                names: vec![ImportedName {
                    name: "annotations".to_string(),
                    alias: None,
                }],
                inline_comment: None,
                line_number: 1,
                is_relative: false,
                is_future: true,
            },
            ImportStatement {
                kind: ImportKind::Import,
                module_path: "os".to_string(),
                names: Vec::new(),
                inline_comment: None,
                line_number: 2,
                is_relative: false,
                is_future: false,
            },
        ];
        let result = validate_future_imports(&block, Path::new("test.py"));
        assert!(result.is_err());
    }
}
