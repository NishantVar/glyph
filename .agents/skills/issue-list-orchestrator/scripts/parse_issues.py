#!/usr/bin/env python3
"""
Markdown-anchored-slices adapter for the Issue-List Orchestrator.

Parses a markdown file shaped like Glyph's `mvp-issues.md` and emits a JSON
list of issues on stdout.

Output schema:
{
  "issues": [
    {
      "id": "1",
      "title": "Workspace bootstrap & walking skeleton",
      "slug": "workspace-bootstrap-and-walking-skeleton",
      "deps": [],
      "acceptance": ["criterion 1", "criterion 2", ...],
      "prose": "free-form prose under '### What to build'",
      "context_files": ["design/foo.md", ...]
    },
    ...
  ]
}

Parsing rules (markdown-anchored-slices format):

- Each top-level "## Slice N — <title>" header starts a new issue.
- Issue ID is N as a string. The em-dash ("—") or hyphen ("-") between the
  number and the title is tolerated.
- Slug is kebab-case from the title.
- Within a slice section, the "### What to build" subsection populates `prose`
  (everything until the next ### or ## header).
- "### Acceptance criteria" populates `acceptance` (markdown checkbox list:
  "- [ ] ...", "- [x] ..."; we strip the box and capture the text).
- Dependencies for slice N are read from a per-slice line like
  "- **Blocked by:** None" or "- **Blocked by:** #1, #2, #3"
  appearing inside the slice section. "None" → empty list. "#N–#M" or "#N-#M"
  ranges (em-dash or hyphen) are expanded inclusively.
- Per-issue context files are read from the document-level "## Per-issue
  context budget" section, which contains a sub-section per slice
  ("### Slice N — ..."). Each bullet under that sub-section is captured;
  bullets shaped like "`design/foo.md` — note" or "design/foo.md (note)"
  contribute the path "design/foo.md" to context_files.
- The pre-slices section "## Per-issue context budget" intro paragraph that
  lists "universal" files is ignored — universal context files are hardcoded
  in SKILL.md (see "Fixed Glyph configuration").

The parser is forgiving: it logs warnings to stderr when something looks off
but does not abort unless the document has zero parseable slices.

Usage:
    python parse_issues.py mvp-issues.md
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path
from typing import Any


# ---------- regexes ----------

# "## Slice N — Title" or "## Slice N - Title"
SLICE_HEADER_RE = re.compile(r"^##\s+Slice\s+(\d+)\s*[—\-–]\s*(.+?)\s*$")
# "### What to build", "### Acceptance criteria", any "###" subsection
SUBSECTION_RE = re.compile(r"^###\s+(.+?)\s*$")
# "## ..." top-level headers (used to know when a slice ends)
TOP_LEVEL_RE = re.compile(r"^##\s+(?!#)")
# "- **Blocked by:** ..."
BLOCKED_BY_RE = re.compile(r"^-?\s*\*\*Blocked by:?\*\*\s*(.+?)\s*$", re.IGNORECASE)
# "#N" extraction
ISSUE_REF_RE = re.compile(r"#(\d+)")
# "#N–#M" or "#N-#M" ranges (em-dash, en-dash, or hyphen)
RANGE_RE = re.compile(r"#(\d+)\s*[–\-—]\s*#(\d+)")
# "- [ ] criterion" or "- [x] criterion"
CHECKBOX_RE = re.compile(r"^-\s+\[[ xX]\]\s+(.+?)\s*$")
# Bullet under per-issue context budget — "- `path` ..." or "- path ..."
CONTEXT_BULLET_RE = re.compile(r"^-\s+(.+?)\s*$")
# Find a path in a context bullet: backtick path, or first whitespace-delimited
# token that contains "/" or ends with ".md"
CONTEXT_PATH_RE = re.compile(r"`([^`]+)`|([^\s,]+\.md)")


# ---------- helpers ----------

def slugify(title: str) -> str:
    """Kebab-case slug; strip backticks/punctuation; collapse runs of '-'."""
    s = title.lower()
    s = re.sub(r"`[^`]+`", lambda m: m.group(0).strip("`"), s)  # unwrap backticks
    s = re.sub(r"[^a-z0-9]+", "-", s)
    s = re.sub(r"-+", "-", s).strip("-")
    return s


def expand_ranges(text: str) -> list[str]:
    """Find #N references and #N-#M ranges, return de-duplicated id list."""
    ids: list[str] = []
    seen: set[str] = set()

    # Expand ranges first.
    def _take_range(m: re.Match) -> str:
        a, b = int(m.group(1)), int(m.group(2))
        lo, hi = (a, b) if a <= b else (b, a)
        for n in range(lo, hi + 1):
            sid = str(n)
            if sid not in seen:
                seen.add(sid)
                ids.append(sid)
        return ""  # consume

    remaining = RANGE_RE.sub(_take_range, text)
    # Now plain #N refs.
    for m in ISSUE_REF_RE.finditer(remaining):
        sid = m.group(1)
        if sid not in seen:
            seen.add(sid)
            ids.append(sid)
    return ids


# ---------- main parser ----------

def parse(text: str) -> dict[str, Any]:
    lines = text.splitlines()
    n = len(lines)

    # First pass: locate all slice section spans.
    # spans[i] = (start_idx, end_idx_exclusive, issue_id, title)
    spans: list[tuple[int, int, str, str]] = []
    pcb_start: int | None = None  # "## Per-issue context budget" line index

    for i, line in enumerate(lines):
        m = SLICE_HEADER_RE.match(line)
        if m:
            spans.append((i, n, m.group(1), m.group(2).strip()))
            continue
        if line.strip().lower().startswith("## per-issue context budget"):
            pcb_start = i

    # Set proper end indices: each slice ends where the next ## (top-level) starts.
    for idx in range(len(spans)):
        start = spans[idx][0]
        next_top = n
        for j in range(start + 1, n):
            if TOP_LEVEL_RE.match(lines[j]):
                next_top = j
                break
        spans[idx] = (start, next_top, spans[idx][2], spans[idx][3])

    if not spans:
        print("error: no '## Slice N — ...' headers found in input", file=sys.stderr)
        sys.exit(2)

    # Parse per-issue context budget into a map: id -> [paths...]
    context_map: dict[str, list[str]] = {}
    if pcb_start is not None:
        # Find pcb end: next top-level "## " or end of doc.
        pcb_end = n
        for j in range(pcb_start + 1, n):
            if TOP_LEVEL_RE.match(lines[j]):
                pcb_end = j
                break
        # Inside pcb, parse "### Slice N — ..." sub-sections.
        cur_id: str | None = None
        for j in range(pcb_start + 1, pcb_end):
            line = lines[j]
            sub = SUBSECTION_RE.match(line)
            if sub:
                # Detect "Slice N — ..."
                hdr = sub.group(1)
                m2 = re.match(r"Slice\s+(\d+)\s*[—\-–]", hdr)
                if m2:
                    new_id: str = m2.group(1) or ""
                    cur_id = new_id
                    context_map.setdefault(new_id, [])
                else:
                    cur_id = None
                continue
            if cur_id is None:
                continue
            bullet = CONTEXT_BULLET_RE.match(line)
            if not bullet:
                continue
            payload = bullet.group(1)
            # Pull first path-looking thing out.
            m3 = CONTEXT_PATH_RE.search(payload)
            if m3:
                path = m3.group(1) or m3.group(2)
                if path and path not in context_map[cur_id]:
                    context_map[cur_id].append(path)

    # Second pass: per-slice extraction.
    issues: list[dict[str, Any]] = []
    for (start, end, iid, title) in spans:
        prose_lines: list[str] = []
        acceptance: list[str] = []
        deps: list[str] = []

        in_prose = False
        in_accept = False
        for j in range(start + 1, end):
            line = lines[j]
            sub = SUBSECTION_RE.match(line)
            if sub:
                name = sub.group(1).strip().lower()
                in_prose = name == "what to build"
                in_accept = name.startswith("acceptance")
                continue
            if in_prose:
                prose_lines.append(line)
                continue
            if in_accept:
                m = CHECKBOX_RE.match(line.strip())
                if m:
                    acceptance.append(m.group(1).strip())
                continue
            # Out of subsection — look for "Blocked by"
            mb = BLOCKED_BY_RE.match(line.strip())
            if mb:
                payload = mb.group(1).strip()
                if payload.lower().startswith("none"):
                    deps = []
                else:
                    # Strip parenthetical commentary only if real refs survive
                    # outside the parens. Two patterns occur in mvp-issues.md:
                    #   (a) "#1 (... #20 ...)"    — refs both outside and
                    #       inside; inside ones are commentary. Strip parens.
                    #   (b) "all relevant slices (#2, #4–#15, ...)" — refs
                    #       only inside; the parens hold the real list.
                    #       Keep parens so expand_ranges sees them.
                    without_parens = re.sub(r"\([^)]*\)", "", payload)
                    if ISSUE_REF_RE.search(without_parens):
                        cleaned = without_parens
                    else:
                        cleaned = payload
                    deps = expand_ranges(cleaned)
                continue

        prose = "\n".join(prose_lines).strip()

        issues.append({
            "id": iid,
            "title": title,
            "slug": slugify(title),
            "deps": deps,
            "acceptance": acceptance,
            "prose": prose,
            "context_files": context_map.get(iid, []),
        })

    # Sanity warnings.
    seen_ids = {i["id"] for i in issues}
    for issue in issues:
        for d in issue["deps"]:
            if d not in seen_ids:
                print(
                    f"warn: slice {issue['id']} depends on #{d} which has no slice header",
                    file=sys.stderr,
                )
        if not issue["acceptance"]:
            print(
                f"warn: slice {issue['id']} has no '### Acceptance criteria' checkboxes",
                file=sys.stderr,
            )
        if not issue["prose"]:
            print(
                f"warn: slice {issue['id']} has empty '### What to build' section",
                file=sys.stderr,
            )

    return {"issues": issues}


def main() -> None:
    if len(sys.argv) != 2:
        print("usage: parse_issues.py <markdown-file>", file=sys.stderr)
        sys.exit(2)
    path = Path(sys.argv[1])
    if not path.is_file():
        print(f"error: not a file: {path}", file=sys.stderr)
        sys.exit(2)
    text = path.read_text(encoding="utf-8")
    result = parse(text)
    json.dump(result, sys.stdout, indent=2, ensure_ascii=False)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
