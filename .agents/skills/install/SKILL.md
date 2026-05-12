---
name: install
description: 'Install Glyph by downloading prebuilt artifacts from the latest GitHub release (Core CLI/LSP, Editor Extensions, and Agent Skills/Commands), falling back to source builds when downloads are unavailable.'
---

## Parameters

- **repo_root**. Default: ".". The local path to a Glyph source clone; consulted only when the release download fails or the user opts for a source build.

## Instructions

### Context

- **installation-phases**

  The installation has three phases: Core CLI/LSP binary, Editor extension (.vsix), and Agent integration (skills + commands). Each phase tries the prebuilt release artifact first and falls back to a source build only if the download cannot be used.

- **release-first-strategy**

  Default to downloading prebuilt artifacts from the latest GitHub release (https://github.com/NishantVar/glyph/releases). Build from source only when a release asset is missing for the user's platform, the download fails, or the user explicitly opts to build.

- **optional-node-deps**

  Node and npm are only needed for the source-build fallback of the editor extension. They are not required when the latest release ships a prebuilt .vsix. The core CLI install must proceed regardless of whether node/npm are present.

### Steps

1. Follow the install-cli-binary procedure below.
2. Default path — download the prebuilt `.vsix` from the latest GitHub release:
   a. List the assets attached to the latest release. Prefer `gh release view --repo NishantVar/glyph --json assets -q '.assets[].name'`; otherwise parse `curl -fsSL https://api.github.com/repos/NishantVar/glyph/releases/latest`.
   b. If a `.vsix` asset is published, download it to a temp directory (using `gh release download` or `curl` as in `install_cli_binary`).
   c. Probe for VS Code-compatible editors: VS Code (`code`), Cursor (`cursor`), Windsurf (`windsurf`), Antigravity (`antigravity`), VSCodium (`codium`). Include OS-specific fallback paths (e.g. `/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code` on macOS).
   d. If any editors are found, ask the user which ones to install the extension into.
   e. For each chosen editor: first run `<cli> --uninstall-extension glyph.glyph-language` to remove any older version (ignore errors if not installed), then run `<cli> --install-extension <vsix_path>`.
   Fallback — build the `.vsix` from source. Use this when the latest release does not include a `.vsix` asset, the download failed, or the user explicitly opts to build:
   a. Verify the source repo is present at `{repo_root}/editors/vscode/package.json`. If missing, instruct the user to clone https://github.com/NishantVar/glyph and rerun with the correct `repo_root`, then skip this phase and continue with the rest of the installation.
   b. Run `command -v node` and `command -v npm`. If either is missing, ask the user for explicit permission to install Node.js (brew on macOS, apt on Linux). If they decline or installation fails, explicitly state that the editor extension build is being skipped and exit this block early so the rest of the workflow continues.
   c. With node and npm available: probe for editors as in the default path, ask the user which to install into, then `cd "{repo_root}/editors/vscode"`, run `npm install` and `npm run package` to produce the `.vsix`, then install into each chosen editor (uninstall the previous version first, as in the default path).
3. Run `command -v glyph` to locate the installed binary (which doubles as the LSP server). If it resolves, print the absolute path so the user can confirm. If it does not resolve, the binary is installed somewhere not on PATH (e.g. `~/.local/bin/glyph` when `~/.local/bin` is not exported, or `~/.cargo/bin/glyph` from a fallback source build). Instruct the user to either add that directory to their shell PATH, or set `glyph.serverPath` in each editor's `settings.json` to the absolute path of the binary.
4. Check the user's home directory for existing configuration directories of popular coding agents:
   a. Gemini CLI: `~/.gemini/`
   b. Claude Code: `~/.claude/`
   c. Codex: `~/.codex/`
   d. Goose: `~/.config/goose/` or similar standard path.
   e. OpenCode / OpenHands.
   Return the list of detected agent configuration directories.
5. Print the detected agent configuration directories from the list found in step 4. Ask the user which of these agents they want to install the Glyph skills and commands into. Accept comma-separated names, "all", or an empty response (defaulting to all). Return the agent directories the user chose to install into.
6. Ask the user how they want to install the optional Glyph commands:
   a. Install all commands with descriptions (automatic routing).
   b. Install all commands without descriptions (manual routing only, saves context).
   c. Do not install optional commands.
   Map their choice to one of the following string values: "with_desc", "no_desc", or "none". Return the chosen commands preference.
7. Resolve the absolute path to a Glyph source tree to install skills from. The install mode is inferred from whether the user already has a local Glyph clone — no separate prompt is asked. If {repo_root} already points at a Glyph clone (verify by checking for `{repo_root}/.agents/skills/glyph/SKILL.md` and `{repo_root}/scripts/install_agent_skills.sh`), use that absolute path; the caller will symlink agent configs into it, and the user is expected to keep the clone on disk and `git pull` it to upgrade. Otherwise, create a temporary working directory (`tmp=$(mktemp -d)`), run `git clone --depth 1 https://github.com/NishantVar/glyph.git "$tmp/glyph"`, and use `$tmp/glyph` as the resolved path (the caller will copy from it; the temp directory may be removed afterward). Never create a persistent clone in a hidden path like `~/.local/share/glyph` — that would leave state the user did not ask for. Return the resolved absolute path to the Glyph source tree for installation.
8. Iterate over each agent directory chosen in step 5. Infer the install mode from the source path resolved in step 7: if its path begins with `/tmp/` or `/var/folders/`, use copy mode; otherwise, use symlink mode (the user brought their own Glyph clone and we will link into it). When the inferred mode is symlink: run `"<source-path>/scripts/install_agent_skills.sh" "<agent_dir>" "<commands-choice>"`, substituting the source path resolved in step 7 and the commands preference from step 6; the script creates the `skills` and `commands` subdirectories if missing and creates symlinks into the source tree — capture and display its output. When the inferred mode is copy: for each agent directory:
   a. Ensure `<agent_dir>/skills/` and `<agent_dir>/commands/` exist (`mkdir -p`).
   b. Replace any existing destination first: `rm -rf "<agent_dir>/skills/glyph"` and (when applicable) `rm -rf "<agent_dir>/commands/glyph"`.
   c. Copy the core skill: `cp -R "<source-path>/.agents/skills/glyph" "<agent_dir>/skills/glyph"`, using the source path resolved in step 7.
   d. Branch on the commands preference from step 6: for "with_desc" run `cp -R "<source-path>/.agents/commands/glyph" "<agent_dir>/commands/glyph"`; for "no_desc" run `cp -R "<source-path>/.agents/commands_no_desc/glyph" "<agent_dir>/commands/glyph"`; for "none" skip the commands copy entirely.
   e. Print which paths were written for that agent.
   After processing all agents, if the inferred mode was copy, tell the user it is safe to remove the temp directory at the source path resolved in step 7. Then provide a concise summary of the installation results across all phases, noting whether each phase used a downloaded release artifact or fell back to a source build.
9. After the summary, list the Glyph entry points the user can now start using, in priority order. The exact phrasing depends on the commands preference from step 6. When the preference is "with_desc" or "no_desc" (slash commands were installed), present these four in this order:
    a. `/glyph:decompile` — convert any existing compiled `.md` skill into a `.glyph` source. Most useful for reading skills: `.glyph` is dramatically more readable than the prose form, so browsing skills feels more like reading code.
    b. `/glyph:teach` — author or edit a `.glyph` skill from scratch (or from a task description). The primary entry point for writing new skills.
    c. `/glyph:compile` — compile a `.glyph` source into the agent-consumable `.md` form.
    d. `/glyph:icompile` — make a targeted edit to a `.glyph` and its paired compiled `.md` in lockstep, without re-running the full compile pipeline.
    Close with a note that the user can also just invoke the `glyph` skill (or `/glyph`) with whatever they want to do and it will route to the right sub-skill. When the preference is "none" (no slash commands installed), present the same four in the same order but as skill names, with phrasing that tells the user to ask their agent to invoke the skill by name:
    a. `glyph:decompile` — same purpose as above; invoke by asking the agent to use the `glyph:decompile` skill on a target `.md`.
    b. `glyph:teach` — invoke by asking the agent to use the `glyph:teach` skill when writing or editing a `.glyph` source.
    c. `glyph:compile` — invoke by asking the agent to use the `glyph:compile` skill on a `.glyph` source.
    d. `glyph:icompile` — invoke by asking the agent to use the `glyph:icompile` skill for paired source-and-compiled edits.
    Close with a note that the user can also just invoke the top-level `glyph` skill with whatever they want to do and it will route to the right sub-skill.

### Constraints

- You must never install missing prerequisites silently. Always ask the user for explicit permission to auto-install missing tools (e.g., via rustup, brew, or apt) before running any installer.
- You must not block the core CLI install or the agent-skills install if node/npm are missing and the user declines to install them. Skip only the editor extension phase in that case.
- Prefer prebuilt release artifacts over source builds whenever a matching asset exists for the user's platform. Source builds are a documented fallback, not the default.

### Procedure: install-cli-binary

1. Default path — download the prebuilt binary from the latest GitHub release. Detect the host platform with `uname -s` and `uname -m` and map to the matching target triple (aarch64-apple-darwin for macOS/ARM, x86_64-apple-darwin for macOS/x86_64, x86_64-unknown-linux-gnu for Linux, x86_64-pc-windows-gnu for Windows/MSYS; if unrecognized, skip to the fallback). Resolve the latest release tag, preferring `gh release view --repo NishantVar/glyph --json tagName -q .tagName` or falling back to `curl -fsSL https://api.github.com/repos/NishantVar/glyph/releases/latest | grep '"tag_name"'`. Download the matching archive `glyph-<tag>-<target>.tar.gz` (or `.zip` on Windows) to a temp directory, preferring `gh release download <tag> --repo NishantVar/glyph --pattern '<archive>' --dir <tmp>` or falling back to `curl`. Extract the archive and move the `glyph` binary into `~/.local/bin/` (`mkdir -p ~/.local/bin` and `chmod +x` as needed). Verify with `~/.local/bin/glyph --version`; if `~/.local/bin` is not on the user's PATH, instruct them to add `export PATH="$HOME/.local/bin:$PATH"` to their shell rc file. Fallback — build from source. Use this only if the download path failed (no matching asset, network failure, unsupported platform) or the user explicitly opts to build. Verify the source repo is present at {repo_root} by checking for `{repo_root}/Cargo.toml` and `{repo_root}/crates/glyph-cli`; if missing, ask the user to clone https://github.com/NishantVar/glyph and rerun, or supply the correct `repo_root`. Run `command -v cargo` to verify Cargo; if missing, ask the user for explicit permission to install via rustup and halt if they decline. Run `cargo install --path crates/glyph-cli` from {repo_root}.
