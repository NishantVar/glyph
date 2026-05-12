---
name: release
description: 'Verify repository state before a release, specifically checking for deviations in the auto-generated commands_no_desc directory and install scripts.'
---

## Parameters

- **repo_root**. The root directory of the repository where the release checks should be executed. Default: ".".

## Instructions

### Context

- **release-context**

  The release pre-flight check ensures that auto-generated directories are synchronized with their sources, and that copied scripts are up-to-date with upstream changes.

### Steps

1. Run `{repo_root}/scripts/sync_commands_no_desc.sh` to regenerate the `.agents/commands_no_desc/glyph` directory.
After running it, use `git status --porcelain {repo_root}/.agents/commands_no_desc/` to check if there are any uncommitted changes in that specific directory.
If there are changes, it means the directory was out of sync. Tell the user exactly which files were modified by the sync script, ask them to commit the changes, and block the release.
If there are no changes, the directory is perfectly in sync.
2. Compare `{repo_root}/scripts/install_agent_skills.sh` with the upstream script located at `~/git/agent-skills/install.sh`.
Check if the upstream file exists using `test -f ~/git/agent-skills/install.sh`. If it doesn't exist, log a warning that parity cannot be verified, but do NOT block the release.
If it exists, run a `diff -u` between the two scripts.
Since `install_agent_skills.sh` contains Glyph-specific modifications (like path resolution), there will naturally be differences.
Your task is to present a summary of the diff to the user (e.g., "The upstream script added support for Goose") alongside the full diff, and ask them to visually confirm if any upstream changes need to be ported over to the Glyph script.
If the user identifies missing upstream changes, block the release and ask them to patch the script.
If the user confirms the script is fine, proceed by returning a success message indicating the repository is clear for release.

### Constraints

- You must block the release if the sync scripts detect any unsynchronized deviations.

