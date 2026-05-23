#!/usr/bin/env bash
# PreToolUse gate for the `implementer` subagent.
#
# Reads the hook payload on stdin. Exits 0 (no-op) for any caller that is NOT
# the implementer subagent, so team-lead and other agents are unaffected.
#
# For the implementer, blocks:
#   - Read/Edit/Write on *.rs (must use graphify / ast-grep / LSP instead)
#   - Bash invocations of escape-hatch / mutating commands not on the allowlist
#
# Allowlisted Bash for the implementer:
#   cargo *, tree-sitter *, ast-grep *, sg *
#
# Git is intentionally NOT on the allowlist. The implementer's verification loop
# is: ast-grep --dry-run (preview) -> ast-grep -U (apply) -> cargo check/nextest.
# Diff inspection is the team-lead's job.
#
# Deny response shape (Claude Code PreToolUse contract):
#   {"hookSpecificOutput": {
#     "hookEventName": "PreToolUse",
#     "permissionDecision": "deny",
#     "permissionDecisionReason": "<why>"}}

set -euo pipefail

payload=$(cat)

agent_type=$(printf '%s' "$payload" | jq -r '.agent_type // empty')
subagent_type=$(printf '%s' "$payload" | jq -r '.subagent_type // empty')
if [ "$agent_type" != "implementer" ] && [ "$subagent_type" != "implementer" ]; then
  exit 0
fi

tool_name=$(printf '%s' "$payload" | jq -r '.tool_name // empty')

deny() {
  local reason=$1
  jq -nc --arg r "$reason" '{
    hookSpecificOutput: {
      hookEventName: "PreToolUse",
      permissionDecision: "deny",
      permissionDecisionReason: $r
    }
  }'
  exit 0
}

case "$tool_name" in
  Read|Edit|Write)
    file_path=$(printf '%s' "$payload" | jq -r '.tool_input.file_path // empty')
    case "$file_path" in
      *.rs)
        deny "Direct ${tool_name} on .rs files is disabled for the implementer. Use graphify (mcp__graphify__*) or ast-grep (mcp__ast-grep__*) to read Rust source, and \`ast-grep run --pattern '...' --rewrite '...' -U <path>\` via Bash to modify it."
        ;;
    esac
    ;;
  Bash)
    command=$(printf '%s' "$payload" | jq -r '.tool_input.command // empty')
    # Strip leading whitespace; take first token for coarse matching, but also
    # inspect the full string for `git <subcommand>` and dangerous substrings.
    trimmed=${command#"${command%%[![:space:]]*}"}
    first=${trimmed%% *}

    # Block obvious escape hatches anywhere in the command.
    case " $command " in
      *" rm "*|*" rm -"*|*"/rm "*|*" sudo "*|*" curl "*|*" wget "*|*" npm "*|*" pnpm "*|*" yarn "*|*" brew "*|*" pip "*|*" pip3 "*|*" python "*|*" python3 "*|*" node "*|*" bash "*|*" sh "*|*" zsh "*|*" eval "*|*" exec "*)
        deny "Bash command contains a disallowed token (rm/sudo/curl/wget/npm/pnpm/yarn/brew/pip/python/node/shell/eval/exec). Implementer Bash is limited to cargo, tree-sitter, and ast-grep. Escalate to the team-lead via SendMessage if you need to run something else."
        ;;
    esac

    case "$first" in
      cargo|tree-sitter|ast-grep|sg)
        exit 0
        ;;
      git)
        deny "git is not available to the implementer. Use \`ast-grep --dry-run\` to preview rewrites and \`cargo check\` / \`cargo nextest\` to verify. Escalate to the team-lead via SendMessage for any git operation."
        ;;
      *)
        deny "Bash command '${first}' is not on the implementer allowlist. Permitted: cargo *, tree-sitter *, ast-grep *, sg *. Escalate to the team-lead for anything else."
        ;;
    esac
    ;;
esac

exit 0
