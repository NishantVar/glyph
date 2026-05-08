---
name: install_glyph_editor_extension
description: Build the Glyph VS Code extension and install it into every VS Code-compatible IDE detected on the user's machine.
---

## Parameters

- **repo_root**. Path to the root of the Glyph repository checkout, used to locate `editors/vscode/` for the build. Default: ".".

## Instructions

### Context

- **extension-source-layout**

  The extension source lives at `editors/vscode/` in the Glyph repo. Its package.json wires `vscode:prepublish` to `npm run bundle` (esbuild bundles src/extension.ts into out/extension.js), and the `package` script runs `vsce package` to emit a self-contained `glyph-language-<version>.vsix`.

- **cli-install-command-shape**

  Every VS Code-compatible CLI accepts the same flags: `<cli> --install-extension <path-to-vsix>` to install, and `<cli> --uninstall-extension <publisher>.<name>` to remove. The Glyph extension identifier is `glyph.glyph-language`.

- **bundled-vsix-is-self-contained**

  The .vsix produced by this build inlines all runtime dependencies via esbuild, so installation does not require node_modules in the editor's extensions directory and works on a fresh clone without any other setup.

### Steps

1. Before building, verify the host has the tools needed to package
the .vsix. Run `command -v node` and `command -v npm`. If either
is missing, tell the user exactly which one is missing and stop
the workflow — do not attempt the build. Suggest installing
Node.js LTS via the platform's standard package manager (Homebrew
on macOS — `brew install node`; apt on Debian/Ubuntu — `sudo apt
install nodejs npm`; the Node.js installer from
https://nodejs.org/ on Windows; or a version manager like `fnm`
or `nvm`). Do not attempt to install Node yourself, since it
requires platform-specific package-manager choices the user must
make. Only proceed past this block when both `node` and `npm`
resolve to executables.
2. Switch the working directory to `{repo_root}/editors/vscode/`. Run
`npm install` to install devDependencies (esbuild, @vscode/vsce)
and the runtime dep (vscode-languageclient); skip when
`node_modules/` already exists and `package-lock.json` is unchanged
since the last install. If install fails, surface the npm error
verbatim and stop. Then run `npm run package`, which invokes
`vscode:prepublish` (esbuild bundles `src/extension.ts` into
`out/extension.js` with every runtime dep inlined) and then runs
`vsce package` to emit a self-contained
`glyph-language-<version>.vsix` next to `package.json`. If either
step fails, surface the exact error and stop. Return the absolute
path to the produced `glyph-language-<version>.vsix` file. Refer to this result as vsix_path.
3. Probe for every VS Code-compatible editor by attempting two checks
per candidate, in order: first run `command -v <cli>` to check
PATH, and if that fails, check the OS-specific fallback path for
the current platform. Record an IDE as detected only when one of
the probes resolves to an executable; skip IDEs that fail both
probes. The candidate set is: VS Code (PATH `code`; macOS fallback
`/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code`;
Windows fallback
`%LOCALAPPDATA%\Programs\Microsoft VS Code\bin\code.cmd`);
Cursor (PATH `cursor`; macOS fallback
`/Applications/Cursor.app/Contents/Resources/app/bin/cursor`;
Windows fallback
`%LOCALAPPDATA%\Programs\cursor\resources\app\bin\cursor.cmd`);
Antigravity (typically not on PATH; macOS fallback
`/Applications/Antigravity.app/Contents/Resources/app/bin/antigravity`
or `~/.antigravity/antigravity/bin/antigravity`; Linux fallback
`~/.antigravity/antigravity/bin/antigravity`); Windsurf (PATH
`windsurf`; macOS fallback
`/Applications/Windsurf.app/Contents/Resources/app/bin/windsurf`);
and VSCodium (PATH `codium`; macOS fallback
`/Applications/VSCodium.app/Contents/Resources/app/bin/codium`).
If the probe yields zero detected IDEs, tell the user no
compatible editor was found, print every CLI name and fallback
path that was probed, and stop the workflow before any install
attempt. Return the list of detected IDE entries — each entry
pairs a display name with the absolute CLI path that subsequent
installs will use. Refer to this result as detected_ides.
4. Print the detected IDEs from detected_ides as a numbered list,
showing each entry's display name and the absolute CLI path that
would be used. Then ask the user which IDEs to install into.
Accept any of: pressing Enter with no input (= every detected IDE),
the bare word `all`, a comma-separated list of indices from the
printed list, or a comma-separated list of IDE display names.
Reject anything else and re-prompt until the input matches one of
these forms. Return the list of IDE entries the user chose;
defaults to every detected IDE when the user accepts the default. Refer to this result as chosen_ides.
5. Iterate over every entry in chosen_ides. For each entry run two
commands in order: first `<cli> --uninstall-extension
glyph.glyph-language` (treat the `extension not installed` error
as success and proceed), then `<cli> --install-extension
vsix_path`. Capture stdout and stderr of both commands. If
install fails, record the exact CLI output for that IDE and
continue with the remaining IDEs — never abort the loop on a
single failure. After the loop completes, print a per-IDE status
table with three columns: IDE display name, install status (ok or
failed), and the captured CLI output for any failure. The table
makes the outcome visible to the user without requiring them to
scroll back through raw command output.
6. Follow the check-glyph-lsp-on-path procedure below.

### Constraints

- Show every detected IDE to the user before triggering any install — never install silently.
- Ask the user which IDEs to install into; default to all detected IDEs when the user accepts the default.
- Run `--uninstall-extension glyph.glyph-language` immediately before every install so re-running the skill produces a clean reinstall instead of leaving a stale cached extension behind.
- After install, check that the glyph LSP binary is reachable on PATH and warn the user if it is missing — without it the extension activates but produces no highlighting.
- You must never run any install or build command under sudo, or ask the user to elevate privileges — every install path here is per-user and runs without root.
- You must never write into any editor's user settings.json from this skill — surface suggestions instead and let the user apply them by hand.

### Procedure: check-glyph-lsp-on-path

1. Run `command -v glyph` to locate the glyph LSP binary. If it
resolves, print the absolute path to the resolved binary so the
user can confirm the LSP server is reachable; no further action
is needed. If it does not resolve, tell the user the glyph CLI
is not on PATH and be explicit: the extension installs cleanly
without it and the language is recognized in editors, but no
syntax highlighting will appear because all highlighting flows
through the LSP server's semantic tokens. Then check for cargo
by running `command -v cargo`. If cargo is available, offer to
run `cargo install --path crates/glyph-cli` from {repo_root} on
the user's behalf and ask them to confirm before running it; on
confirmation, execute the install and report the path to the
newly-installed binary, or surface the cargo error verbatim and
stop on failure. If cargo is not available, tell the user Rust
is required to build the LSP and suggest installing it via
rustup from https://rustup.rs/ before re-running this skill.
Always also describe the alternative remediation: set
`glyph.serverPath` in each editor's user settings.json to the
absolute path of an existing glyph binary. Do not modify
settings.json from this skill — only describe the change.

