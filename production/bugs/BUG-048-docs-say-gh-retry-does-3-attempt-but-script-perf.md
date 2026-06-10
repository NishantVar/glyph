# BUG-048: Docs say gh_retry does '3-attempt' but script performs 4 attempts

**Severity:** low | **Confidence:** high | **Status:** confirmed
**Location:** `.agents/skills/issue-list-orchestrator/SKILL.md:328`
**Found by:** gap:bundled-skill-tooling-scripts | **Audit date:** unknown-date

## Description

`SKILL.md`'s script pointer table states `gh_retry.sh` "Wraps `gh` with 3-attempt 1s/4s/16s backoff", but the script sets `max_attempts=4` and its own header comment (line 10) correctly says "Up to 4 attempts" (1 initial call + 3 retries with 1s/4s/16s delays). The doc undercounts the attempts by one, which is misleading when a maintainer reasons about total worst-case latency or retry behavior.

## Trigger / Reproduction

A maintainer reads the SKILL.md table expecting 3 total attempts but the script actually runs up to 4. Worst-case latency is `1 + 4 + 16 = 21s` of sleep plus 4 `gh` invocations, not 3.

## Evidence

```bash
# .agents/skills/issue-list-orchestrator/scripts/gh_retry.sh

# Line 10 header comment:
# - Up to 4 attempts.
# - Sleeps 1s after attempt 1, 4s after attempt 2, 16s after attempt 3
#   (no sleep after attempt 4).

attempt=1
max_attempts=4          # 4 total attempts
delays=(1 4 16)         # delays before attempts 2, 3, 4
```

```md
<!-- .agents/skills/issue-list-orchestrator/SKILL.md line 328 -->
| `scripts/gh_retry.sh <gh-args...>` | Wraps `gh` with 3-attempt 1s/4s/16s backoff |
```

The table says "3-attempt" but `max_attempts=4` and the script's own header say "Up to 4 attempts."

## Recommended Resolution

Change the table text in `SKILL.md` line 328 to accurately describe the script:

```md
| `scripts/gh_retry.sh <gh-args...>` | Wraps `gh` with 4-attempt (1 initial + 3 retries) 1s/4s/16s backoff |
```

## Verification Notes

`max_attempts=4` in the script and "Up to 4 attempts" in the script's own header comment are both authoritative. The SKILL.md table is the sole location of the discrepancy. Runtime behavior is unaffected; this is a documentation-only defect.
