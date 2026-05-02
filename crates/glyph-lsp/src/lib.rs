//! Glyph Language Server — v1 (M1 milestone).
//!
//! This crate implements a stdio-based LSP server that wraps `glyph-core`'s
//! Phase 1 (Parse) + Phase 2 (Analyze) phases and republishes the resulting
//! `DiagBag` as LSP `publishDiagnostics` notifications.
//!
//! M1 scope (per `design/glyph-lsp.md` §8):
//! - Lifecycle: `initialize`, `initialized`, `shutdown`, `exit`.
//! - Document sync: `didOpen`, `didChange`, `didClose`, `didSave`.
//! - Diagnostics: republished on `didOpen` and `didSave`. `didChange` updates
//!   the in-memory buffer text but does **not** re-lint (per the team-lead's
//!   call on §10.C — save-only diagnostics for v1).
//!
//! NOT in M1: `textDocument/definition` (requires the glyph-core span +
//! resolution work spec'd in §4.3 and §4.4 of the design — that's M2).
//!
//! ## How to run
//!
//! ```ignore
//! // From an editor (e.g., nvim-lspconfig):
//! //   cmd = { "glyph", "lsp" }
//! //
//! // Or directly:
//! //   glyph-lsp
//! //
//! // The server speaks JSON-RPC framed per the LSP spec on stdin/stdout.
//! ```
//!
//! ## Architecture
//!
//! All shared state lives in [`Backend`], which is what `tower-lsp` dispatches
//! method calls on. The state shape — `Arc<RwLock<HashMap<Url, Document>>>` for
//! open buffers — comes from `design/glyph-lsp.md` §5. Diagnostic conversion
//! is in [`convert`]; it is the only place where the inclusive-vs-exclusive
//! end-column gotcha (§10.B) is handled, and it carries a unit test pinning
//! that behaviour.

pub mod convert;

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result as JsonRpcResult;
use tower_lsp::lsp_types::{
    Diagnostic as LspDiagnostic, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DidSaveTextDocumentParams, InitializeParams, InitializeResult,
    InitializedParams, MessageType, ServerCapabilities, ServerInfo, TextDocumentSyncCapability,
    TextDocumentSyncKind, Url,
};
use tower_lsp::{Client, LanguageServer, LspService, Server};

/// One open buffer's mirror inside the LSP server.
///
/// Holds the text we last received from the client plus the diagnostics we
/// last published for the URI. Tracking the published set lets us avoid
/// re-publishing identical bags (a quiet optimisation; mostly it's defensive
/// against editors that flicker on duplicate publishes).
#[derive(Debug, Clone)]
struct Document {
    text: String,
    /// Most recently published diagnostics for this URI, in publish order.
    /// Purely an optimisation hint; correctness does not depend on it.
    last_published: Vec<LspDiagnostic>,
}

/// Server initialization options.
///
/// Mirrors the `--enable-effects` CLI flag. Sent by the editor under
/// `initializationOptions` in the `initialize` request.
#[derive(Debug, Default, Clone, Copy, serde::Deserialize)]
#[serde(default)]
struct InitOptions {
    /// Enable the gated `effects:` subsystem in the compiler. Matches the
    /// `glyph --enable-effects` CLI flag. Default `false`, matching the CLI.
    #[serde(rename = "enableEffects")]
    enable_effects: bool,
}

/// The LSP backend. `tower-lsp` dispatches LSP method calls on this struct.
pub struct Backend {
    client: Client,
    documents: Arc<RwLock<HashMap<Url, Document>>>,
    /// `enable_effects` is set once at `initialize` time from `initializationOptions`
    /// and read on every diagnostic computation. Wrapped in an `RwLock` so the
    /// `initialize` handler (which takes `&self`) can write without the borrow
    /// checker complaining; in practice it's written exactly once.
    enable_effects: Arc<RwLock<bool>>,
}

impl Backend {
    fn new(client: Client) -> Self {
        Self {
            client,
            documents: Arc::new(RwLock::new(HashMap::new())),
            enable_effects: Arc::new(RwLock::new(false)),
        }
    }

    /// Run Phases 1+2 on `text` and publish the resulting diagnostics for `uri`.
    ///
    /// Updates the `last_published` field on the matching `Document` so the
    /// next `did_close` knows what to clear.
    async fn lint_and_publish(&self, uri: Url, text: &str) {
        let enable_effects = *self.enable_effects.read().await;
        let label = uri.path().to_string();
        let bag = glyph_core::check_source_with_effects(text, 0, &label, enable_effects);
        let diagnostics: Vec<LspDiagnostic> =
            bag.sorted().iter().map(convert::diagnostic_to_lsp).collect();

        // Stamp the Document with the publish set so close()/future publishes
        // can compare. Done before publishing so we never lose track of what
        // the client last saw.
        {
            let mut docs = self.documents.write().await;
            if let Some(doc) = docs.get_mut(&uri) {
                doc.last_published = diagnostics.clone();
            }
        }

        self.client
            .publish_diagnostics(uri, diagnostics, None)
            .await;
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> JsonRpcResult<InitializeResult> {
        // Pull `enableEffects` out of `initializationOptions`, if present.
        // Missing or malformed → keep the default (`false`).
        if let Some(raw) = params.initialization_options {
            if let Ok(opts) = serde_json::from_value::<InitOptions>(raw) {
                *self.enable_effects.write().await = opts.enable_effects;
            }
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                // Full text sync: we replace the buffer on every change. Glyph
                // source files are small (kilobytes), so the bandwidth saving
                // from incremental sync is not worth the implementation cost
                // (per `design/glyph-lsp.md` §5).
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                // Definition provider is M2; declare nothing here in M1 so
                // editors don't enable a `gd` mapping that would just return
                // `null`.
                definition_provider: None,
                ..ServerCapabilities::default()
            },
            server_info: Some(ServerInfo {
                name: "glyph-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _params: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "glyph-lsp initialized")
            .await;
    }

    async fn shutdown(&self) -> JsonRpcResult<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let text = params.text_document.text;
        {
            let mut docs = self.documents.write().await;
            docs.insert(
                uri.clone(),
                Document {
                    text: text.clone(),
                    last_published: Vec::new(),
                },
            );
        }
        self.lint_and_publish(uri, &text).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        // Save-only diagnostics in v1 (design §10.C). We update the buffer
        // text so didSave / didClose see the latest content, but we do NOT
        // re-lint here.
        let uri = params.text_document.uri;
        let mut docs = self.documents.write().await;
        let Some(doc) = docs.get_mut(&uri) else { return };

        // tower-lsp's content_changes is `Vec<TextDocumentContentChangeEvent>`.
        // With Full sync (advertised in initialize), each event has `range == None`
        // and `text` is the new full document. If a client somehow sends incremental
        // edits anyway, take the last full-replace if any, else leave text alone.
        for change in params.content_changes {
            if change.range.is_none() {
                doc.text = change.text;
            }
            // Else: incremental change under Full sync — ignore. The next
            // didSave will resync from the editor's authoritative copy.
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri;
        // `text` is sent only when the server requested it via `save: { include_text: true }`
        // in capabilities. We did not, so prefer the cached buffer (which
        // didChange has been keeping fresh). If the cache is missing for any
        // reason, fall back to the save payload.
        let text_opt = {
            let docs = self.documents.read().await;
            docs.get(&uri).map(|d| d.text.clone())
        };
        let text = match (text_opt, params.text) {
            (Some(t), _) => t,
            (None, Some(t)) => t,
            (None, None) => return, // No source available — nothing to lint.
        };
        self.lint_and_publish(uri, &text).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let had_diagnostics = {
            let mut docs = self.documents.write().await;
            let removed = docs.remove(&uri);
            removed.map(|d| !d.last_published.is_empty()).unwrap_or(false)
        };

        // Clear stale squiggles in the editor by publishing an empty array.
        // Skip the publish when the last set was already empty (small
        // optimisation — avoids spurious notifications).
        if had_diagnostics {
            self.client.publish_diagnostics(uri, Vec::new(), None).await;
        }
    }
}

/// Run the LSP server over stdio until the client sends `exit`.
///
/// Builds a `current_thread` `tokio` runtime (per design §10.F — keeps the
/// dependency footprint lean given our serial work pattern) and drives a
/// `tower-lsp::Server` on `stdin`/`stdout`.
pub fn run_stdio() -> std::io::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    runtime.block_on(async {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();
        let (service, socket) = LspService::new(Backend::new);
        Server::new(stdin, stdout, socket).serve(service).await;
    });

    Ok(())
}

