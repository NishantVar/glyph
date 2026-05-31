#!/usr/bin/env bash
# Multi-agent PreToolUse gate. Switches policies by agent_type.
#
# Payload fields used:
#   .agent_type    — set to the subagent_type for spawned agents; empty for team-lead
#   .tool_name     — Bash | Read | Edit | Write
#   .tool_input.*  — tool-specific (command, file_path, etc.)
#
# Policies:
#   team-lead (agent_type empty)  — pass-through (no extra restrictions)
#   implementer                    — current implementer rules (see implementer-gate.sh)
#   reviewer                       — strict read-only
#   *                              — default-allow with a log line
set -euo pipefail
payload=$(cat)
echo "$payload" >> /tmp/payload-dump.jsonl 2>/dev/null || true

agent_type=$(printf '%s' "$payload" | jq -r '.agent_type // empty')
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

# ─────────────────────────────────────────────────────────────────────────────
# Team-lead: pass-through
# ─────────────────────────────────────────────────────────────────────────────
if [ -z "$agent_type" ]; then
  exit 0
fi

# ─────────────────────────────────────────────────────────────────────────────
# Implementer policy
# ─────────────────────────────────────────────────────────────────────────────
implementer_policy() {
  case "$tool_name" in
    Read|Edit|Write)
      file_path=$(printf '%s' "$payload" | jq -r '.tool_input.file_path // empty')
      case "$file_path" in
        *.rs)
          deny "[implementer] Direct ${tool_name} on .rs files is disabled. Use graphify/ast-grep to read, and \`ast-grep run --pattern '...' --rewrite '...' -U <path>\` via Bash to modify."
          ;;
      esac
      ;;
    Bash)
      command=$(printf '%s' "$payload" | jq -r '.tool_input.command // empty')
      trimmed=${command#"${command%%[![:space:]]*}"}
      first=${trimmed%% *}
      case " $command " in
        *" rm "*|*" rm -"*|*"/rm "*|*" sudo "*|*" curl "*|*" wget "*|*" npm "*|*" pnpm "*|*" yarn "*|*" brew "*|*" pip "*|*" pip3 "*|*" python "*|*" python3 "*|*" node "*|*" bash "*|*" sh "*|*" zsh "*|*" eval "*|*" exec "*)
          deny "[implementer] Disallowed token (rm/sudo/curl/wget/npm/pnpm/yarn/brew/pip/python/node/shell/eval/exec)."
          ;;
      esac
      case "$first" in
        cargo|tree-sitter|ast-grep|sg) exit 0 ;;
        git) deny "[implementer] git is not available. Use ast-grep --dry-run and cargo check/nextest." ;;
        *)   deny "[implementer] Bash '${first}' not on allowlist (cargo, tree-sitter, ast-grep, sg)." ;;
      esac
      ;;
  esac
}

# ─────────────────────────────────────────────────────────────────────────────
# Reviewer policy: strict read-only
# ─────────────────────────────────────────────────────────────────────────────
reviewer_policy() {
  case "$tool_name" in
    Edit|Write)
      deny "[reviewer] Reviewer is read-only. Cannot Edit/Write. Escalate via SendMessage if changes are needed."
      ;;
    Bash)
      command=$(printf '%s' "$payload" | jq -r '.tool_input.command // empty')
      trimmed=${command#"${command%%[![:space:]]*}"}
      first=${trimmed%% *}
      case " $command " in
        *" rm "*|*" sudo "*|*" curl "*|*" wget "*|*" >>"*|*" > "*|*" tee "*|*" eval "*|*" exec "*)
          deny "[reviewer] Disallowed token (rm/sudo/curl/wget/redirect/tee/eval/exec)."
          ;;
      esac
      case "$first" in
        ls|cat|grep|rg|find|wc|head|tail|file|stat|tree|diff|jq|awk|sed) exit 0 ;;
        git)
          subcmd=$(printf '%s' "$command" | awk '{print $2}')
          case "$subcmd" in
            status|diff|log|show|branch|blame|rev-parse|ls-files|describe) exit 0 ;;
            *) deny "[reviewer] git ${subcmd} not allowed (only read-only subcommands)." ;;
          esac
          ;;
        *) deny "[reviewer] Bash '${first}' not on read-only allowlist." ;;
      esac
      ;;
  esac
}

case "$agent_type" in
  implementer) implementer_policy ;;
  reviewer)    reviewer_policy ;;
  *)           exit 0 ;;
esac

exit 0
