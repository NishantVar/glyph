// Glyph VS Code extension entry point.
//
// Spawns `<glyph.serverPath> lsp` (default `glyph lsp`) over stdio and
// hands the connection to vscode-languageclient. The server provides:
//   - publishDiagnostics on save (and on open) for `.glyph.md` buffers
//   - textDocument/definition (same-file + cross-file imports)
//   - textDocument/semanticTokens/full (M3)
//
// The extension is intentionally minimal — all language behaviour lives
// in `glyph-lsp` so VS Code and Neovim share the same logic.

import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  const config = vscode.workspace.getConfiguration("glyph");
  const serverPath = config.get<string>("serverPath", "glyph");
  const enableEffects = config.get<boolean>("enableEffects", false);

  const serverOptions: ServerOptions = {
    run: {
      command: serverPath,
      args: ["lsp"],
      transport: TransportKind.stdio,
    },
    debug: {
      command: serverPath,
      args: ["lsp"],
      transport: TransportKind.stdio,
    },
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      { scheme: "file", language: "glyph" },
      { scheme: "file", pattern: "**/*.glyph.md" },
    ],
    initializationOptions: {
      enableEffects,
    },
    synchronize: {
      // Re-lint when an edit is saved to a `.glyph.md` file outside the
      // currently active editor (e.g., import target edited in another
      // pane). The save-only behavior matches design §10.C.
      fileEvents: vscode.workspace.createFileSystemWatcher("**/*.glyph.md"),
    },
  };

  client = new LanguageClient(
    "glyph",
    "Glyph Language Server",
    serverOptions,
    clientOptions,
  );

  try {
    await client.start();
  } catch (err) {
    vscode.window.showErrorMessage(
      `Failed to start the Glyph language server (\`${serverPath} lsp\`): ${
        err instanceof Error ? err.message : String(err)
      }. Set \`glyph.serverPath\` to a valid \`glyph\` binary.`,
    );
    return;
  }

  context.subscriptions.push({
    dispose: () => {
      void client?.stop();
    },
  });
}

export async function deactivate(): Promise<void> {
  if (!client) {
    return;
  }
  await client.stop();
  client = undefined;
}
