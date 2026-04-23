//! Typing-related import detection for Python import sorting.
//!
//! Stdlib vs third-party classification is delegated to ruff; this module
//! only identifies type-annotation imports (`typing`, `typing_extensions`,
//! `collections.abc`) so they can be pinned to the bottom of the stdlib
//! section. Ordering within that group is handled by the natural
//! length-first segment sort in [`crate::sorter::compare_module_path`].

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
