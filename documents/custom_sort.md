# Custom Import Sorting — Design Discussion

## The Specification

From `PURE-PYTHON-UV.md`, imports within each section must be sorted by these rules:

1. `import ...` statements come before `from ... import ...` statements.
2. Sort module paths **level-by-level** (segment by segment, separated by `.`).
3. **Length-based segment comparison**: a shorter segment ranks before a longer one at the same level; if two segments have the same length, use alphabetical order as the tiebreaker.
4. **Parent before child**: when one path is the prefix of another (e.g. `a.b` vs `a.b.c`), the parent always comes first.
5. **Imported names** within a `from ... import a, b, c` statement are also sorted by length first, then alphabetically.
6. Type-annotation-related imports (`typing`, `typing_extensions`, `collections.abc`) are always placed at the **bottom of the standard library group** (no blank line separating them from the rest of stdlib).
7. **No multi-name `import` statements**: `import a, b` is not supported. The tool should raise an error and instruct the user to run `ruff format` first to split these into separate statements.
8. **`__future__` imports** are preserved at the top of each section. The tool assumes imports have already been sectioned per PEP 8; if `__future__` imports are found mixed with non-`__future__` imports in the same contiguous block, the tool should raise an error.
9. **Star imports** (`from module import *`): `*` is treated as a single name with length 1 for sorting purposes.
10. **Comments above imports stay in place**: inline comments on the same line as an import travel with the import, but comment-only lines above an import do **not** move. This practice (comment-above-import) is discouraged.
11. **Aliased imports** (`import numpy as np`, `from os.path import join as j`): the sort key uses the **original name**, not the alias.
12. **Relative imports** (`from . import foo`, `from ..bar import baz`): belong to the local section. Within a section, all relative imports (starting with `.`) sort **before** absolute imports. Within each of those two groups, sort by the standard length-first rules.
13. **Backslash continuations** are not supported. The tool should raise an error and instruct the user to run `ruff format` first to convert them to parenthesized form.
14. **Multiple import blocks**: the tool sorts each contiguous block of first-level import statements independently. Non-import code (assignments, function calls, etc.) acts as a block boundary.
15. **Guarded imports** (`if TYPE_CHECKING:`, `try/except ImportError`, or any `if` guard): imports inside these blocks are **ignored** — not sorted and not validated.

### Example from the spec

Imports are sorted **within each section** (stdlib, third-party, local). Sections are separated by a blank line.

```python
# --- Standard library ---
import os
import abc
import csv
import datetime
from xml.sax.handler import ContentHandler
from xml.parsers.expat import ExpatError
from collections import defaultdict
from collections.abc import Mapping
from typing import Any, Dict, List, Optional

# --- Third-party / local ---
from a.b import c
from a.b.c import d
from a.b.add import f
from a.bc.a import k
```

**Notes on this example:**
- `os` (2) sorts before `abc` (3) because length takes priority over alphabetical order.
- `xml.sax.handler` sorts before `xml.parsers.expat`: at segment 2, `sax` (3) < `parsers` (7) by length, so segment 3 is never compared.
- `collections.abc` and `typing` are pinned to the bottom of the stdlib section as type-annotation-related imports.
- Imported names within `from typing import ...` are sorted by length then alphabetically: `Any` (3), `Dict` (4), `List` (4), `Optional` (8).

---

## Key Finding: Alphabetical Sorting Does Not Approximate This Spec

A natural question is whether standard alphabetical sorting (used by isort and ruff) produces roughly the same result. **It does not**, even for common standard library modules.

The length-based rule causes short module names to sort *before* longer ones regardless of their alphabetical position. Examples:

| Spec order | isort/alphabetical order | Why they differ |
|---|---|---|
| `os` before `collections` | `collections` before `os` | `os` (2) < `collections` (11), but `c` < `o` |
| `re` before `csv` | `csv` before `re` | `re` (2) < `csv` (3), but `c` < `r` |
| `io` before `ast` | `ast` before `io` | `io` (2) < `ast` (3), but `a` < `i` |
| `os` before `sys` | `os` before `sys` | Coincidentally agree: 2 < 3 AND `o` < `s` |

Any import block containing both short modules (`os`, `re`, `io`, `gc`) and longer ones (`abc`, `csv`, `ast`, `json`) will diverge — which is essentially every standard library import block.

---

## The Sorting Algorithm

The custom comparator operates on module path strings:

```
compare(a, b):
  for each (segment_a, segment_b) pair at the same level:
    if len(segment_a) != len(segment_b): return shorter first
    if segment_a != segment_b:           return alphabetical order
  return the path with fewer segments first (parent before child)
```

In Rust pseudocode:

```rust
fn compare_module_path(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let a_segs: Vec<&str> = a.split('.').collect();
    let b_segs: Vec<&str> = b.split('.').collect();

    for (sa, sb) in a_segs.iter().zip(b_segs.iter()) {
        let by_len = sa.len().cmp(&sb.len());
        if by_len != Ordering::Equal { return by_len; }
        let by_alpha = sa.cmp(sb);
        if by_alpha != Ordering::Equal { return by_alpha; }
    }
    a_segs.len().cmp(&b_segs.len()) // parent before child
}
```

---

## Tool Options Evaluated

### Option A — isort / ruff configuration (Rejected)

isort and ruff sort imports alphabetically and do not support a pluggable segment comparator. Specifically:

- **`import` before `from`**: supported (default behavior).
- **Length-based segment comparison**: **not supported**. No configuration knob exists for this.
- **`from typing import` at bottom of stdlib section**: no clean solution. A custom `TYPING` section in isort/ruff works, but it inserts a blank line before `from typing import`, violating the "same section" requirement.

**Verdict**: Cannot implement the spec via configuration alone.

### Option B — Fork ruff (Not recommended)

ruff is written in Rust and its isort sorting logic lives around `crates/ruff_linter/src/rules/isort/sorting.rs`. However:

- The comparator change itself is small (~20–50 lines), but the infrastructure cost is high:
  - ruff releases very frequently (multiple times per week).
  - Maintaining a fork means owning the diff against a fast-moving upstream indefinitely.
  - You would need to build and distribute a custom `ruff` binary instead of using `uvx ruff`.

**Verdict**: Small code change, large ongoing infrastructure and maintenance cost.

### Option C — Standalone Rust import sorter (Recommended)

Write a small, focused tool in Rust that implements only import sorting to this spec, used alongside stock ruff (which handles linting and formatting). No upstream maintenance burden.

**Component breakdown**:

| Component | Difficulty | Priority | Notes |
|---|---|---|---|
| Python import parsing | Moderate–Hard | v1 | Use `rustpython-parser` crate; must handle multi-line parenthesized imports, inline comments, `# noqa`/`# type: ignore` annotations, backslash continuations, and `if TYPE_CHECKING:` blocks. Multi-name `import a, b` statements should be rejected with an error directing the user to run `ruff format` first. |
| Stdlib vs third-party classification | Easy | v1 | Embed ~300-entry `HashSet` of stdlib module names |
| Custom sort algorithm | Easy | v1 | Simple `Ord` implementation as shown above |
| Type-annotation import pinning | Easy | v1 | Special-case `typing`, `typing_extensions`, `collections.abc` within stdlib group sort |
| Source reconstruction | Hard | v1 | Preserving comments, blank lines, multi-line imports; byte-range replacement |
| CLI with `--check`/`--diff` modes | Easy | v1 | See CLI Interface section below |
| `--stdin` mode for editor integration | Easy | v2 | Pipe-based interface for editor plugins |

The hard part is source reconstruction — correctly writing back the reordered imports while preserving surrounding context (comments between imports, blank lines, `if TYPE_CHECKING:` blocks, etc.).

**Integration**:
```
uvx ruff check .          # linting
uvx ruff format .         # formatting
your-import-sorter .      # custom sort
```

### Option D — Open an upstream PR to ruff

Propose a `length-sort-segments` flag in `[tool.ruff.lint.isort]`. This is the cleanest long-term outcome but unlikely to be accepted given how niche the use case is.

---

## CLI Interface

The tool should be named `lort` (or configurable). It operates in two modes: **fix** (default) and **check**.

```
lort [OPTIONS] [PATHS...]
```

### Arguments

| Argument | Description |
|---|---|
| `PATHS...` | Files or directories to process. Defaults to `.` (current directory). Directories are searched recursively for `*.py` files. |

### Options

| Flag | Short | Description |
|---|---|---|
| `--check` | `-c` | Check mode: report violations without modifying files. Exit code 1 if any file is unsorted. |
| `--diff` | `-d` | Show a unified diff of what would change (implies `--check`). |
| `--quiet` | `-q` | Suppress all output except errors. |
| `--verbose` | `-v` | Show each file processed and whether it was modified/already sorted. |
| `--exclude` | `-e` | Glob patterns to exclude (e.g. `--exclude "migrations/*"`). |
| `--stdin` | | Read from stdin, write sorted result to stdout. Useful for editor integration. |
| `--help` | `-h` | Print help. |
| `--version` | `-V` | Print version. |

### Exit Codes

| Code | Meaning |
|---|---|
| `0` | Success: all files are sorted (or were fixed in fix mode). |
| `1` | Check mode: one or more files have unsorted imports. |
| `2` | Error: parse failure, multi-name `import` statement found, I/O error, etc. |

### Error Behavior

When the tool encounters an unsupported construct (e.g. `import a, b`), it should:
1. Print an error message identifying the file, line, and the problem.
2. Suggest the fix: `Run 'ruff format' to split multi-name imports.`
3. Exit with code 2. Do **not** partially fix the file.

### Integration Examples

**Pre-commit hook** (`.pre-commit-config.yaml`):
```yaml
- repo: local
  hooks:
    - id: lort
      name: lort import sorter
      entry: lort --check
      language: system
      types: [python]
```

**CI check**:
```bash
lort --check --diff .
```

**Editor (stdin mode)**:
```bash
cat myfile.py | lort --stdin
```

---

## Test Corpus

The following test cases should be implemented to validate the sorting algorithm. Each case exercises a specific rule or edge case.

### TC-01: Basic length-before-alpha ordering

```python
# Input
import collections
import os
import re
import csv

# Expected
import os
import re
import csv
import collections
```

### TC-02: `import` before `from` within the same section

```python
# Input
from os.path import join
import os
import sys

# Expected
import os
import sys
from os.path import join
```

### TC-03: Segment-by-segment comparison

```python
# Input
from xml.parsers.expat import ExpatError
from xml.sax.handler import ContentHandler

# Expected
from xml.sax.handler import ContentHandler
from xml.parsers.expat import ExpatError
```

*Rationale*: Segment 2: `sax` (3) < `parsers` (7) by length.

### TC-04: Parent before child

```python
# Input
from a.b.c import d
from a.b import c

# Expected
from a.b import c
from a.b.c import d
```

### TC-05: Imported names sorted by length then alpha

```python
# Input
from typing import Optional, Any, List, Dict

# Expected
from typing import Any, Dict, List, Optional
```

*Rationale*: `Any` (3) < `Dict` (4) = `List` (4) (alpha tiebreak: `D` < `L`) < `Optional` (8).

### TC-06: Type-annotation imports pinned to bottom of stdlib

```python
# Input
from typing import Any
from collections.abc import Mapping
import os
import csv
from collections import defaultdict

# Expected
import os
import csv
from collections import defaultdict
from collections.abc import Mapping
from typing import Any
```

### TC-07: Multi-name `import` statement rejected

```python
# Input
import os, sys

# Expected: ERROR
# "Line 1: Multi-name import statement found (`import os, sys`).
#  Run 'ruff format' to split multi-name imports."
```

### TC-08: Single-segment modules with same length

```python
# Input
import sys
import ast
import csv
import abc
import os
import re

# Expected
import os
import re
import abc
import ast
import csv
import sys
```

*Rationale*: Length groups: (2) `os`, `re`; (3) `abc`, `ast`, `csv`, `sys` — alphabetical within each group.

### TC-09: Deeply nested paths

```python
# Input
from a.bc.a import k
from a.b.add import f
from a.b.c import d
from a.b import c

# Expected
from a.b import c
from a.b.c import d
from a.b.add import f
from a.bc.a import k
```

*Rationale*:
- `a.b` is parent of `a.b.c` and `a.b.add` → comes first.
- `a.b.c` (seg3 len 1) < `a.b.add` (seg3 len 3).
- `a.bc.a`: seg2 `bc` (2) > `b` (1) → sorts last.

### TC-10: Already-sorted file produces no changes

```python
# Input (already correct)
import os
import abc
import csv

# Expected: identical output, exit code 0 in --check mode
```

### TC-11: Star import sorts by length (length 1)

```python
# Input
from os.path import join, exists
from os import *

# Expected
from os import *
from os.path import join, exists
```

*Rationale*: `*` has length 1, sorting it among other names. But here it's a different module path so it's ordered by path first: `os` (1 segment) before `os.path` (2 segments).

### TC-12: Aliased imports sort by original name

```python
# Input
from os.path import join as j, abspath as ap

# Expected
from os.path import join as j, abspath as ap
```

*Rationale*: `join` (4) < `abspath` (7) by length. The aliases `j` and `ap` are ignored for sorting.

### TC-13: Relative imports before absolute within local section

```python
# Input
from mypackage.utils import helper
from . import foo
from ..bar import baz

# Expected
from . import foo
from ..bar import baz
from mypackage.utils import helper
```

*Rationale*: Relative imports (starting with `.`) come before absolute imports in the local section.

### TC-14: Guarded imports are ignored

```python
# Input
import os
import abc

if TYPE_CHECKING:
    import zlib
    import ast

# Expected (only the top-level block is sorted; guarded block untouched)
import os
import abc

if TYPE_CHECKING:
    import zlib
    import ast
```

### TC-15: `__future__` mixed with non-`__future__` is an error

```python
# Input
from __future__ import annotations
import os

# Expected: ERROR
# "__future__ imports must appear in their own block before other imports."
```

### TC-16: Backslash continuation rejected

```python
# Input
from os.path import \
    join, exists

# Expected: ERROR
# "Line 1: Backslash continuation in import statement.
#  Run 'ruff format' to convert to parenthesized form."
```

### TC-17: Multiple independent import blocks

```python
# Input
import csv
import os

logger = logging.getLogger(__name__)

import json
import re

# Expected
import os
import csv

logger = logging.getLogger(__name__)

import re
import json
```

*Rationale*: Each contiguous import block is sorted independently. The assignment acts as a block boundary.

### TC-18: Inline comment travels with import

```python
# Input
import csv  # for data parsing
import os

# Expected
import os
import csv  # for data parsing
```

### TC-19: Comment-only line above import stays in place

```python
# Input
# Important module:
import csv
import os

# Expected
# Important module:
import os
import csv
```

*Rationale*: The comment `# Important module:` stays at its original position; it does not travel with `import csv`.

---

## Resolved Design Decisions

| Topic | Decision |
|---|---|
| `if` guards (`TYPE_CHECKING`, `try/except`) | Ignore — do not sort or validate imports inside guards |
| Multi-line parenthesized imports | Supported |
| Inline comments on import lines | Supported — travel with the import |
| Comment-only lines above imports | Stay in place (discouraged practice) |
| `__future__` imports | Preserved at top; error if mixed with non-`__future__` in same block |
| Star imports (`from x import *`) | `*` treated as name with length 1 |
| Aliased imports | Sort by original name, not alias |
| Relative imports | Local section; relative before absolute within section |
| Backslash continuations | Rejected — user must run `ruff format` first |
| Multiple import blocks | Each contiguous block sorted independently |
