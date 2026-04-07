//! Typing-related import prioritization for Python import sorting.
//!
//! Stdlib vs third-party classification is delegated to ruff; this module
//! only handles pinning type-annotation imports (`typing`, `typing_extensions`,
//! `collections.abc`) to the bottom of the stdlib section.

/// Module paths whose `from` imports are treated as type-annotation-related
/// and pinned to the bottom of the stdlib section.
const TYPING_MODULES: &[&str] = &["typing", "typing_extensions", "collections.abc"];

/// Check whether an import path is a type-annotation-related module
/// that should be pinned to the bottom of the stdlib section.
///
/// # Arguments
///
/// * `module_path` - The full dotted module path (e.g. `typing`, `collections.abc`).
///
/// # Returns
///
/// `true` if the path matches one of the known typing-related modules.
pub fn is_typing_related(module_path: &str) -> bool {
    TYPING_MODULES.contains(&module_path)
}

/// Return a priority value for typing-related modules within the
/// typing group.
///
/// `typing` itself is always last (highest priority value).
/// Non-typing modules get 0 (they are sorted normally elsewhere).
///
/// # Returns
///
/// A `u8` priority: 0 = not typing, 1 = other typing modules,
/// 2 = `typing` itself (always at the very bottom).
pub fn typing_priority(module_path: &str) -> u8 {
    if module_path == "typing" {
        2
    } else if is_typing_related(module_path) {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_typing_related() {
        assert!(is_typing_related("typing"));
        assert!(is_typing_related("typing_extensions"));
        assert!(is_typing_related("collections.abc"));
    }

    #[test]
    fn test_not_typing_related() {
        assert!(!is_typing_related("collections"));
        assert!(!is_typing_related("os"));
        assert!(!is_typing_related("typing.io"));
    }
}
