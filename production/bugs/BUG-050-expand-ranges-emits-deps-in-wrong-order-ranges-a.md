# BUG-050: expand_ranges emits deps in wrong order: ranges always sorted ahead of earlier plain refs

**Severity:** low | **Confidence:** high | **Status:** confirmed
**Location:** `.agents/skills/issue-list-orchestrator/scripts/parse_issues.py:97-120`
**Found by:** gap:bundled-skill-tooling-scripts | **Audit date:** unknown-date

## Description

`expand_ranges` runs `RANGE_RE.sub(...)` to expand all `#N-#M` ranges FIRST (appending their ids to `ids`), then scans the leftover text for plain `#N` refs. Because the range substitution removes the matched range text before the plain-ref pass, a plain `#N` that appears BEFORE a range in the source is appended AFTER the range's expanded ids. Concrete trigger from the shipped mvp-issues.md: `**Blocked by:** all relevant feature slices (#2, #4-#15, #19, #20, #21)` produces deps `['4','5',...,'15','2','19','20','21']` — `#2`, which is first in the source, lands after the range. The emitted JSON `deps` order therefore does not match source order. Any consumer that displays or relies on dep ordering sees scrambled lists.

## Trigger / Reproduction

Input string: `#2, #4-#15, #19, #20, #21`

Expected output (source order): `['2', '4', '5', '6', '7', '8', '9', '10', '11', '12', '13', '14', '15', '19', '20', '21']`

Actual output: `['4', '5', '6', '7', '8', '9', '10', '11', '12', '13', '14', '15', '2', '19', '20', '21']`

`#2` lands at position 12 because `RANGE_RE.sub` processes the range first, appending 4..15 to `ids`, then the remaining string `#2, , #19, #20, #21` is scanned by the plain-ref pass.

## Evidence

```python
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
```

## Recommended Resolution

Process refs in a single left-to-right pass so source order is preserved: iterate with a combined regex that matches either a range or a single `#N` (e.g. `re.finditer(r"#(\d+)\s*[–\-—]\s*#(\d+)|#(\d+)", text)`), expanding a range when groups 1/2 are present and appending the single id otherwise, deduping via the existing `seen` set.

## Verification Notes

The two-pass approach is confirmed to produce out-of-source-order output. The `deps` field is used as a set for topological dependency checking (all deps must be `merged` before an issue is `ready`), so ordering is irrelevant to the actual consumer behavior — making this low severity. The proposed single-pass combined regex fix correctly produces source-order output.
