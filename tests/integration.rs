//! Integration tests implementing TC-01 through TC-19 from the spec.
//!
//! Each test feeds a Python source string through the full pipeline
//! (parse -> sort -> reconstruct) and asserts the expected output.

use std::path::PathBuf;

use lort::parser::parse_file;
use lort::sorter::sort_and_reconstruct;

/// Helper: run the full pipeline and return the sorted source.
fn sort_source(source: &str) -> Result<String, lort::error::LortError> {
    let path = PathBuf::from("test.py");
    let segments = parse_file(source, &path)?;
    sort_and_reconstruct(segments, &path)
}

/// Helper: assert that sorting produces the expected output.
fn assert_sorts_to(input: &str, expected: &str) {
    let result = sort_source(input).expect("should not error");
    assert_eq!(
        result, expected,
        "\n--- got ---\n{result}--- expected ---\n{expected}"
    );
}

/// Helper: assert that sorting produces an error containing `needle`.
fn assert_sort_errors(input: &str, needle: &str) {
    let result = sort_source(input);
    assert!(result.is_err(), "expected error but got Ok");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains(needle),
        "expected error containing '{needle}', got: {err}"
    );
}

// --- TC-01: Basic length-before-alpha ordering ---

#[test]
fn tc01_basic_length_before_alpha() {
    assert_sorts_to(
        "import collections\nimport os\nimport re\nimport csv\n",
        "import os\nimport re\nimport csv\nimport collections\n",
    );
}

// --- TC-02: `import` before `from` within the same section ---

#[test]
fn tc02_import_before_from() {
    assert_sorts_to(
        "from os.path import join\nimport os\nimport sys\n",
        "import os\nimport sys\nfrom os.path import join\n",
    );
}

// --- TC-03: Segment-by-segment comparison ---

#[test]
fn tc03_segment_by_segment() {
    assert_sorts_to(
        "from xml.parsers.expat import ExpatError\nfrom xml.sax.handler import ContentHandler\n",
        "from xml.sax.handler import ContentHandler\nfrom xml.parsers.expat import ExpatError\n",
    );
}

// --- TC-04: Parent before child ---

#[test]
fn tc04_parent_before_child() {
    assert_sorts_to(
        "from a.b.c import d\nfrom a.b import c\n",
        "from a.b import c\nfrom a.b.c import d\n",
    );
}

// --- TC-05: Imported names sorted by length then alpha ---

#[test]
fn tc05_imported_names_sorted() {
    assert_sorts_to(
        "from typing import Optional, Any, List, Dict\n",
        "from typing import Any, Dict, List, Optional\n",
    );
}

// --- TC-06: Type-annotation imports pinned to bottom of stdlib ---

#[test]
fn tc06_typing_pinned_to_bottom() {
    assert_sorts_to(
        "from typing import Any\nfrom collections.abc import Mapping\nimport os\nimport csv\nfrom collections import defaultdict\n",
        "import os\nimport csv\nfrom collections import defaultdict\nfrom typing import Any\nfrom collections.abc import Mapping\n",
    );
}

#[test]
fn tc06b_typing_group_order() {
    assert_sorts_to(
        "from typing_extensions import Protocol\nfrom collections.abc import Mapping\nfrom typing import Any\nimport os\n",
        "import os\nfrom typing import Any\nfrom collections.abc import Mapping\nfrom typing_extensions import Protocol\n",
    );
}

// --- TC-07: Multi-name `import` statement rejected ---

#[test]
fn tc07_multi_name_import_rejected() {
    assert_sort_errors("import os, sys\n", "Multi-name import");
}

// --- TC-08: Single-segment modules with same length ---

#[test]
fn tc08_same_length_alpha_tiebreak() {
    assert_sorts_to(
        "import sys\nimport ast\nimport csv\nimport abc\nimport os\nimport re\n",
        "import os\nimport re\nimport abc\nimport ast\nimport csv\nimport sys\n",
    );
}

// --- TC-09: Deeply nested paths ---

#[test]
fn tc09_deeply_nested() {
    assert_sorts_to(
        "from a.bc.a import k\nfrom a.b.add import f\nfrom a.b.c import d\nfrom a.b import c\n",
        "from a.b import c\nfrom a.b.c import d\nfrom a.b.add import f\nfrom a.bc.a import k\n",
    );
}

// --- TC-10: Already-sorted file produces no changes ---

#[test]
fn tc10_already_sorted() {
    let input = "import os\nimport abc\nimport csv\n";
    assert_sorts_to(input, input);
}

// --- TC-11: Star import sorts by length (length 1) ---

#[test]
fn tc11_star_import() {
    assert_sorts_to(
        "from os.path import join, exists\nfrom os import *\n",
        "from os import *\nfrom os.path import join, exists\n",
    );
}

// --- TC-12: Aliased imports sort by original name ---

#[test]
fn tc12_aliased_imports() {
    assert_sorts_to(
        "from os.path import join as j, abspath as ap\n",
        "from os.path import join as j, abspath as ap\n",
    );
}

// --- TC-13: Relative imports before absolute within local section ---

#[test]
fn tc13_relative_before_absolute() {
    assert_sorts_to(
        "from mypackage.utils import helper\nfrom . import foo\nfrom ..bar import baz\n",
        "from . import foo\nfrom ..bar import baz\nfrom mypackage.utils import helper\n",
    );
}

#[test]
fn tc13b_relative_sorted_by_dot_count() {
    // 2-dot relative would sort before 1-dot under pure length-first rules.
    assert_sorts_to(
        "from ...deep import C\nfrom ..parent import B\nfrom .local import A\nfrom absolute_module import X\n",
        "from .local import A\nfrom ..parent import B\nfrom ...deep import C\nfrom absolute_module import X\n",
    );
}

#[test]
fn tc13c_relative_same_dots_length_first() {
    assert_sorts_to(
        "from .collections import X\nfrom .os import Y\nfrom .abc import Z\nfrom absolute_module import W\n",
        "from .os import Y\nfrom .abc import Z\nfrom .collections import X\nfrom absolute_module import W\n",
    );
}

#[test]
fn tc13d_relative_and_absolute_mixed_with_import_subsection() {
    assert_sorts_to(
        "import collections\nimport os\nfrom ...deep import C\nfrom absolute_first import X\nfrom ..parent import B\nfrom .local import A\nfrom another_absolute import Y\n",
        "import os\nimport collections\nfrom .local import A\nfrom ..parent import B\nfrom ...deep import C\nfrom absolute_first import X\nfrom another_absolute import Y\n",
    );
}

// --- TC-14: Guarded imports are ignored ---

#[test]
fn tc14_guarded_imports_ignored() {
    let input = "import os\nimport abc\n\nif TYPE_CHECKING:\n    import zlib\n    import ast\n";
    let expected = "import os\nimport abc\n\nif TYPE_CHECKING:\n    import zlib\n    import ast\n";
    assert_sorts_to(input, expected);
}

// --- TC-15: __future__ mixed with non-__future__ is an error ---

#[test]
fn tc15_future_mixed_error() {
    assert_sort_errors(
        "from __future__ import annotations\nimport os\n",
        "__future__",
    );
}

// --- TC-16: Backslash continuation rejected ---

#[test]
fn tc16_backslash_rejected() {
    assert_sort_errors(
        "from os.path import \\\n    join, exists\n",
        "Backslash continuation",
    );
}

// --- TC-17: Multiple independent import blocks ---

#[test]
fn tc17_multiple_blocks() {
    assert_sorts_to(
        "import csv\nimport os\n\nlogger = logging.getLogger(__name__)\n\nimport json\nimport re\n",
        "import os\nimport csv\n\nlogger = logging.getLogger(__name__)\n\nimport re\nimport json\n",
    );
}

// --- TC-18: Inline comment travels with import ---

#[test]
fn tc18_inline_comment_travels() {
    assert_sorts_to(
        "import csv  # for data parsing\nimport os\n",
        "import os\nimport csv  # for data parsing\n",
    );
}

// --- TC-19: Comment-only line above import stays in place ---

#[test]
fn tc19_comment_above_stays() {
    assert_sorts_to(
        "# Important module:\nimport csv\nimport os\n",
        "# Important module:\nimport os\nimport csv\n",
    );
}

// --- Additional edge case: empty file ---

#[test]
fn edge_empty_file() {
    let result = sort_source("").expect("should not error");
    assert_eq!(result, "");
}

// --- Additional edge case: file with no imports ---

#[test]
fn edge_no_imports() {
    assert_sorts_to("x = 1\ny = 2\n", "x = 1\ny = 2\n");
}

// --- Additional edge case: single-line parenthesized import ---

#[test]
fn edge_single_line_parenthesized() {
    assert_sorts_to(
        "from typing import (Optional, Any, Dict)\n",
        "from typing import Any, Dict, Optional\n",
    );
}

// --- Multi-line parenthesized imports preserve format ---

#[test]
fn edge_multiline_parenthesized_preserved() {
    assert_sorts_to(
        "from sync_tools.sync import (\n\
         \x20   execute_sync_plan,\n\
         \x20   build_copy_plan,\n\
         \x20   display_sync_plan,\n\
         )\n",
        "from sync_tools.sync import (\n\
         \x20   build_copy_plan,\n\
         \x20   display_sync_plan,\n\
         \x20   execute_sync_plan,\n\
         )\n",
    );
}

#[test]
fn edge_multiline_mixed_with_single_line() {
    assert_sorts_to(
        "from sync_tools.sync import (\n\
         \x20   execute_sync_plan,\n\
         \x20   build_copy_plan,\n\
         )\n\
         from sync_tools.models import SyncMode, SyncOperation\n",
        "from sync_tools.sync import (\n\
         \x20   build_copy_plan,\n\
         \x20   execute_sync_plan,\n\
         )\n\
         from sync_tools.models import SyncMode, SyncOperation\n",
    );
}

// --- Plain `import X as Y` alias preservation ---

#[test]
fn edge_plain_import_alias_preserved() {
    // Basic aliased import should preserve the alias
    assert_sorts_to("import polars as pl\n", "import polars as pl\n");
}

#[test]
fn edge_plain_import_alias_sorts_by_module_name() {
    // Aliased imports sort by module path, not alias
    assert_sorts_to(
        "import tensorflow as tf\nimport polars as pl\nimport typer\n",
        "import typer\nimport polars as pl\nimport tensorflow as tf\n",
    );
}

#[test]
fn edge_plain_import_alias_with_inline_comment() {
    // Aliased import with inline comment
    assert_sorts_to(
        "import polars as pl  # data processing\nimport os\n",
        "import os\nimport polars as pl  # data processing\n",
    );
}

#[test]
fn edge_plain_import_alias_already_sorted() {
    // Multiple aliased imports already in correct order
    assert_sorts_to(
        "import os\nimport polars as pl\nimport tensorflow as tf\n",
        "import os\nimport polars as pl\nimport tensorflow as tf\n",
    );
}

#[test]
fn edge_plain_import_no_alias_to_alias() {
    // Plain import without alias, then with alias - should preserve alias in second
    assert_sorts_to(
        "import polars as pl\nimport os\n",
        "import os\nimport polars as pl\n",
    );
}

#[test]
fn edge_mixed_plain_and_from_aliases_round_trip() {
    // Mixed alias forms should both survive parse -> sort -> reconstruct.
    assert_sorts_to(
        "from os.path import join as j, abspath as ap\nimport polars as pl\nimport os\n",
        "import os\nimport polars as pl\nfrom os.path import join as j, abspath as ap\n",
    );
}
