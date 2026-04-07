# lort

A Python import sorter that uses **length-first segment ordering** — shorter module-path segments sort before longer ones, with alphabetical order as a tiebreaker. The result is a staircase-like visual effect that is easy to scan.

**Note:** lort is designed to run *after* `ruff format` (which resolves constructs lort cannot parse) and *alongside* `ruff check`.

## Sorting Rules

Imports within each section are sorted by:

1. `import ...` statements come before `from ... import ...` statements.
2. Module paths are compared **segment by segment** (split on `.`). Please find more details about the sort key in the next paragraph.
3. **Length-based segment comparison**: a shorter segment ranks before a longer one at the same level; equal-length segments use alphabetical order.
4. **Parent before child**: `a.b` always comes before `a.b.c`.
5. **Imported names** within a `from ... import a, b, c` statement are sorted by length, then alphabetically.
6. Type-annotation imports (`typing`, `typing_extensions`, `collections.abc`) are pinned to the **bottom of their contiguous import block**.
7. **Relative imports** sort before absolute imports within the local section.
8. **`__future__` imports** must appear in their own contiguous block (not mixed with other imports) and are pinned to the top.

The sort key for a module path is computed segment by segment (pseudo code):

```
compare(a, b):
  for each (segment_a, segment_b) pair at the same level:
    if len(segment_a) != len(segment_b): return shorter first
    if segment_a != segment_b:           return alphabetical order
  return the path with fewer segments first  (parent before child)
```

This produces a different ordering from isort/ruff (which sort alphabetically). For example, `os` sorts before `collections` because `"os"` (2 characters) is shorter than `"collections"` (11 characters), even though `c` < `o` alphabetically.

See [`documents/custom_sort.md`](documents/custom_sort.md) for the full design document, including the rationale for a standalone tool over forking ruff.

### Comparison with isort

isort (and ruff's import sorter) orders imports **alphabetically** within each section. lort orders imports by **segment length first**, then alphabetically as a tiebreaker.

| Feature | isort / ruff | lort |
|---|---|---|
| Primary sort key | Alphabetical | Segment length (shorter first) |
| Tiebreaker | — | Alphabetical |
| `import` before `from` | Yes (default) | Yes |
| Type-annotation imports (`typing`, etc.) | No special treatment | Pinned to bottom of import block |
| Section grouping (stdlib / third-party / local) | Yes | Not handled (use ruff) |
| Relative imports | Configurable | Always before absolute in local section |

**Example — same imports, different order:**

```python
# isort output (alphabetical)
from __future__ import annotations
import collections
import os
from typing import Any, Dict, List, Optional
from xml.parsers.expat import ExpatError
from xml.sax.handler import ContentHandler

# lort output (length-first)
from __future__ import annotations

import os
import collections
from xml.sax.handler import ContentHandler
from xml.parsers.expat import ExpatError
from typing import Any, Dict, List, Optional
```

Key differences in this example:
- `__future__` imports must be in their own contiguous block in lort (separated by a blank line); isort handles this automatically within its section logic.
- `os` (2 chars) sorts before `collections` (11 chars) in lort; alphabetically `c` < `o` so isort puts `collections` first.
- `xml.sax` sorts before `xml.parsers` in lort because `sax` (3) < `parsers` (7) at the second segment; isort puts `xml.parsers` first because `p` < `s`.
- `typing` is pinned to the bottom of the import block in lort; isort places it alphabetically among other imports.

## Installation

### From source (requires Rust)

```bash
cargo install --git https://github.com/ceshine/lort
```

### Pre-built binaries

We may provide this installation option in the future.

### Automatic installation via Pre-commit/Prek

See the "Pre-commit Hook" section below.

## Usage

```bash
lort [OPTIONS] [PATHS...]
```

Fix imports in-place (default mode):

```bash
lort .
lort src/
lort myfile.py
```

Check without modifying (exit code 1 if any file is unsorted):

```bash
lort --check .
```

Show a unified diff of what would change:

```bash
lort --diff .
```

Read from stdin, write to stdout:

```bash
cat myfile.py | lort --stdin
```

### Options

| Flag | Short | Description |
|---|---|---|
| `--check` | `-c` | Check mode: report violations without modifying files. Exit 1 if any file is unsorted. |
| `--diff` | `-d` | Show a unified diff of what would change (implies `--check`). |
| `--quiet` | `-q` | Suppress all output except errors. |
| `--verbose` | `-v` | Show each file processed and whether it was modified/already sorted. |
| `--exclude PATTERN` | `-e` | Pattern to exclude (repeatable, e.g. `-e "migrations/*"`). A leading `*` matches any prefix; a trailing `*` matches any substring before it; otherwise matches as a substring. |
| `--stdin` | | Read from stdin, write sorted result to stdout. |
| `--help` | `-h` | Print help. |
| `--version` | `-V` | Print version. |

### Exit Codes

| Code | Meaning |
|---|---|
| `0` | All files are sorted (or were fixed in fix mode). |
| `1` | Check mode: one or more files have unsorted imports. |
| `2` | Parse error, unsupported construct, or I/O error. |

### Unsupported Constructs

lort rejects two constructs and asks you to run `ruff format` first:

- **Multi-name imports**: `import os, sys` — run `ruff format` to split into separate statements.
- **Backslash continuations**: `from os.path import \` — run `ruff format` to convert to parenthesized form.

Imports inside guards (`if TYPE_CHECKING:`, `try/except ImportError`, other `if` blocks) are left untouched.

## Pre-commit Hook

### Using the hook from GitHub (recommended)

Add to your `.pre-commit-config.yaml`:

```yaml
repos:
  - repo: https://github.com/ceshine/lort
    rev: v0.1.0  # replace with the latest tag
    hooks:
      - id: lort        # check mode (recommended for CI)
      # - id: lort-fix  # fix mode (rewrites files in place)
```

Two hook IDs are available:

| Hook ID | Behavior |
|---|---|
| `lort` | Check mode — fails if any file has unsorted imports (does not modify files). |
| `lort-fix` | Fix mode — rewrites files with sorted imports. |

pre-commit's `rust` language support builds the binary from source on first run using `cargo`. Rust must be installed on the machine (or available in the pre-commit environment). The first build may take a minute or two.

### Local hook (binary already on PATH)

If you have installed `lort` manually and prefer to call it directly:

```yaml
repos:
  - repo: local
    hooks:
      - id: lort
        name: lort import sorter (check)
        entry: lort --check
        language: system
        types: [python]
```

Or in fix mode:

```yaml
repos:
  - repo: local
    hooks:
      - id: lort-fix
        name: lort import sorter (fix)
        entry: lort
        language: system
        types: [python]
```

## Integration with ruff

lort is intended to be used alongside ruff, not as a replacement:

```bash
uvx ruff format .   # formatting (also fixes multi-name imports and backslash continuations)
uvx ruff check .    # linting
lort .              # length-first import sorting
```

Run `ruff format` before `lort` so that any unsupported constructs are resolved first.

## AI Use Disclosure

- The initial design document and codebase were created with Claude (Opus 4.6).
- This README was written with the assistance of Claude (Sonnet 4.6).
- Some of the commit messages were created by OpenCode (various open models).
