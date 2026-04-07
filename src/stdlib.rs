use std::sync::LazyLock;

use std::collections::HashSet;

/// Set of top-level Python 3.12 standard library module names.
///
/// Used to classify imports into stdlib vs third-party sections.
/// Sourced from `sys.stdlib_module_names` in CPython 3.12.
// NOTE for Rust newcomers: `LazyLock` is like Python's module-level constant
// that gets initialized once on first access. It's the idiomatic way to have
// a "lazy static" in Rust without external crates.
static STDLIB_MODULES: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    HashSet::from([
        "__future__",
        "_thread",
        "abc",
        "aifc",
        "argparse",
        "array",
        "ast",
        "asynchat",
        "asyncio",
        "asyncore",
        "atexit",
        "audioop",
        "base64",
        "bdb",
        "binascii",
        "binhex",
        "bisect",
        "builtins",
        "bz2",
        "calendar",
        "cgi",
        "cgitb",
        "chunk",
        "cmath",
        "cmd",
        "code",
        "codecs",
        "codeop",
        "collections",
        "colorsys",
        "compileall",
        "concurrent",
        "configparser",
        "contextlib",
        "contextvars",
        "copy",
        "copyreg",
        "cProfile",
        "crypt",
        "csv",
        "ctypes",
        "curses",
        "dataclasses",
        "datetime",
        "dbm",
        "decimal",
        "difflib",
        "dis",
        "distutils",
        "doctest",
        "email",
        "encodings",
        "enum",
        "errno",
        "faulthandler",
        "fcntl",
        "filecmp",
        "fileinput",
        "fnmatch",
        "fractions",
        "ftplib",
        "functools",
        "gc",
        "getopt",
        "getpass",
        "gettext",
        "glob",
        "grp",
        "gzip",
        "hashlib",
        "heapq",
        "hmac",
        "html",
        "http",
        "idlelib",
        "imaplib",
        "imghdr",
        "imp",
        "importlib",
        "inspect",
        "io",
        "ipaddress",
        "itertools",
        "json",
        "keyword",
        "lib2to3",
        "linecache",
        "locale",
        "logging",
        "lzma",
        "mailbox",
        "mailcap",
        "marshal",
        "math",
        "mimetypes",
        "mmap",
        "modulefinder",
        "multiprocessing",
        "netrc",
        "nis",
        "nntplib",
        "numbers",
        "operator",
        "optparse",
        "os",
        "ossaudiodev",
        "pathlib",
        "pdb",
        "pickle",
        "pickletools",
        "pipes",
        "pkgutil",
        "platform",
        "plistlib",
        "poplib",
        "posix",
        "posixpath",
        "pprint",
        "profile",
        "pstats",
        "pty",
        "pwd",
        "py_compile",
        "pyclbr",
        "pydoc",
        "queue",
        "quopri",
        "random",
        "re",
        "readline",
        "reprlib",
        "resource",
        "rlcompleter",
        "runpy",
        "sched",
        "secrets",
        "select",
        "selectors",
        "shelve",
        "shlex",
        "shutil",
        "signal",
        "site",
        "smtpd",
        "smtplib",
        "sndhdr",
        "socket",
        "socketserver",
        "spwd",
        "sqlite3",
        "sre_compile",
        "sre_constants",
        "sre_parse",
        "ssl",
        "stat",
        "statistics",
        "string",
        "stringprep",
        "struct",
        "subprocess",
        "sunau",
        "symtable",
        "sys",
        "sysconfig",
        "syslog",
        "tabnanny",
        "tarfile",
        "telnetlib",
        "tempfile",
        "termios",
        "test",
        "textwrap",
        "threading",
        "time",
        "timeit",
        "tkinter",
        "token",
        "tokenize",
        "tomllib",
        "trace",
        "traceback",
        "tracemalloc",
        "tty",
        "turtle",
        "turtledemo",
        "types",
        "typing",
        "typing_extensions",
        "unicodedata",
        "unittest",
        "urllib",
        "uu",
        "uuid",
        "venv",
        "warnings",
        "wave",
        "weakref",
        "webbrowser",
        "winreg",
        "winsound",
        "wsgiref",
        "xdrlib",
        "xml",
        "xmlrpc",
        "zipapp",
        "zipfile",
        "zipimport",
        "zlib",
        "zoneinfo",
    ])
});

/// Module paths whose `from` imports are treated as type-annotation-related
/// and pinned to the bottom of the stdlib section.
const TYPING_MODULES: &[&str] = &["typing", "typing_extensions", "collections.abc"];

/// Check whether a top-level module name belongs to the Python standard library.
///
/// # Arguments
///
/// * `module_path` - The full dotted module path (e.g. `os.path`, `collections.abc`).
///   Only the first segment is checked.
///
/// # Returns
///
/// `true` if the first segment is a known stdlib module.
pub fn is_stdlib(module_path: &str) -> bool {
    // Extract the top-level module name (first segment before any dot).
    let top_level = module_path.split('.').next().unwrap_or(module_path);
    STDLIB_MODULES.contains(top_level)
}

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
    fn test_stdlib_modules() {
        assert!(is_stdlib("os"));
        assert!(is_stdlib("os.path"));
        assert!(is_stdlib("collections"));
        assert!(is_stdlib("collections.abc"));
        assert!(is_stdlib("typing"));
        assert!(is_stdlib("xml.parsers.expat"));
    }

    #[test]
    fn test_non_stdlib_modules() {
        assert!(!is_stdlib("requests"));
        assert!(!is_stdlib("numpy"));
        assert!(!is_stdlib("mypackage.utils"));
    }

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
