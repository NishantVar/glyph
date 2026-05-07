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
//! M2 scope adds `textDocument/definition` (per §7) backed by
//! `glyph_core::analyze::analyze_with_resolutions` (§4.4) plus a follow-the-
//! imports walk for cross-file targets. Both same-file and cross-file
//! jumps are wired up in M2.
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
    DidOpenTextDocumentParams, DidSaveTextDocumentParams, GotoDefinitionParams,
    GotoDefinitionResponse, InitializeParams, InitializeResult, InitializedParams, Location,
    MessageType, OneOf, SemanticToken, SemanticTokenModifier as LspTokenModifier,
    SemanticTokenType as LspTokenType, SemanticTokens, SemanticTokensFullOptions,
    SemanticTokensLegend, SemanticTokensOptions, SemanticTokensParams, SemanticTokensResult,
    SemanticTokensServerCapabilities, ServerCapabilities, ServerInfo,
    TextDocumentSyncCapability, TextDocumentSyncKind, Url, WorkDoneProgressOptions,
};
use tower_lsp::{Client, LanguageServer, LspService, Server};

use glyph_core::analyze::ResolutionKind;
use glyph_core::ast::{Decl, FlowStmt, ReturnExpr};
use glyph_core::semantic_tokens::{
    collect_semantic_tokens, RawSemToken, SemTokenModifier, SemTokenType,
};
use glyph_core::span::{LineIndex, Span};

/// One open buffer's mirror inside the LSP server.
///
/// Holds the text we last received from the client plus the diagnostics we
/// last published for the URI. Tracking the published set lets us avoid
/// re-publishing identical bags (a quiet optimisation; mostly it's defensive
/// against editors that flicker on duplicate publishes).
#[derive(Debug, Clone, Default)]
struct Document {
    text: String,
    /// Most recently published diagnostics for this URI, in publish order.
    /// Purely an optimisation hint; correctness does not depend on it.
    last_published: Vec<LspDiagnostic>,
    /// URIs of imported dep files we last published cross-file diagnostics for
    /// from this buffer's lint. On the next lint we clear any URI that drops
    /// out of the new set so stale dep diagnostics don't linger in the editor.
    /// Buffer's own URI is excluded.
    last_dep_uris: std::collections::HashSet<Url>,
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

    /// Run Phases 1+2 on `text` (with import-aware cross-file diagnostics, M3)
    /// and publish the per-file `DiagBag`s. The buffer's URI always gets a
    /// publish; each imported dep with diagnostics gets its own publish under
    /// that dep's `file://` URI.
    ///
    /// Cross-file invalidation: any dep URI we published for last time but
    /// that's not in the current import set gets an empty publish (clears
    /// stale squiggles in the editor when an `import` line changes).
    async fn lint_and_publish(&self, uri: Url, text: &str) {
        let enable_effects = *self.enable_effects.read().await;

        // Convert the buffer URI to a filesystem path so import resolution
        // works against the importer's directory. Untitled buffers
        // (`untitled:`, `inmemory:`) won't have a `file://` path; in that
        // case fall back to an empty path and we'll publish single-file
        // diagnostics under the buffer URI only.
        let buffer_path = uri
            .to_file_path()
            .unwrap_or_else(|_| std::path::PathBuf::from(uri.path()));

        let bags =
            glyph_core::check_source_with_imports(text, 0, &buffer_path, enable_effects);

        // Resolve which key in `bags` corresponds to the buffer. The core
        // canonicalizes the entry path when possible; match against both.
        let canon_buffer = buffer_path
            .canonicalize()
            .unwrap_or_else(|_| buffer_path.clone());

        let mut new_dep_uris: std::collections::HashSet<Url> =
            std::collections::HashSet::new();
        let mut buffer_diagnostics: Vec<LspDiagnostic> = Vec::new();

        for (path, bag) in bags.iter() {
            let diagnostics: Vec<LspDiagnostic> =
                bag.sorted().iter().map(convert::diagnostic_to_lsp).collect();

            let is_buffer = path == &canon_buffer || path == &buffer_path;
            if is_buffer {
                buffer_diagnostics = diagnostics;
                continue;
            }

            // Cross-file dep — publish under the dep's URI.
            let dep_uri = match Url::from_file_path(path) {
                Ok(u) => u,
                Err(_) => continue, // unrepresentable as `file://` — skip
            };
            new_dep_uris.insert(dep_uri.clone());
            self.client
                .publish_diagnostics(dep_uri, diagnostics, None)
                .await;
        }

        // Clear any dep URI we published for last time that's no longer
        // referenced by this buffer.
        let stale_dep_uris: Vec<Url> = {
            let docs = self.documents.read().await;
            docs.get(&uri)
                .map(|d| {
                    d.last_dep_uris
                        .iter()
                        .filter(|u| !new_dep_uris.contains(u))
                        .cloned()
                        .collect()
                })
                .unwrap_or_default()
        };
        for stale in stale_dep_uris {
            self.client.publish_diagnostics(stale, Vec::new(), None).await;
        }

        // Stamp the Document with this lint's published set so we know
        // what to clear next time.
        {
            let mut docs = self.documents.write().await;
            if let Some(doc) = docs.get_mut(&uri) {
                doc.last_published = buffer_diagnostics.clone();
                doc.last_dep_uris = new_dep_uris;
            }
        }

        self.client
            .publish_diagnostics(uri, buffer_diagnostics, None)
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
                // M2: same-file go-to-def. Cross-file is M3 (we still
                // advertise the capability — the M3 patch will extend
                // resolution to follow imports without changing this flag).
                definition_provider: Some(OneOf::Left(true)),
                // M3: semantic tokens (`textDocument/semanticTokens/full`).
                // We advertise the legend from
                // `glyph_core::semantic_tokens::{SemTokenType, SemTokenModifier}`
                // and serve only the `full` request — `range` and `delta`
                // would buy little for files this small.
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            work_done_progress_options: WorkDoneProgressOptions::default(),
                            legend: SemanticTokensLegend {
                                token_types: SemTokenType::legend()
                                    .iter()
                                    .map(|s| LspTokenType::new(s))
                                    .collect(),
                                token_modifiers: SemTokenModifier::legend()
                                    .iter()
                                    .map(|s| LspTokenModifier::new(s))
                                    .collect(),
                            },
                            range: Some(false),
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                        },
                    ),
                ),
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
                    last_dep_uris: std::collections::HashSet::new(),
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
        let (had_diagnostics, dep_uris) = {
            let mut docs = self.documents.write().await;
            let removed = docs.remove(&uri);
            match removed {
                Some(d) => (!d.last_published.is_empty(), d.last_dep_uris),
                None => (false, std::collections::HashSet::new()),
            }
        };

        // Clear stale squiggles in the editor by publishing an empty array.
        // Skip the publish when the last set was already empty (small
        // optimisation — avoids spurious notifications).
        if had_diagnostics {
            self.client.publish_diagnostics(uri, Vec::new(), None).await;
        }
        // Clear cross-file dep diagnostics that were attributed to this
        // buffer. If another open buffer also imports the same dep, its next
        // save will re-publish — so we'd clear here and another buffer would
        // re-attach. Acceptable in M3 (the design's "FileGraph cache" would
        // sharpen this; deferred per the M3 brief).
        for dep in dep_uris {
            self.client.publish_diagnostics(dep, Vec::new(), None).await;
        }
    }

    /// `textDocument/definition` — go-to-def per design §7.
    ///
    /// Algorithm: parse + analyze + resolve the buffer (following imports for
    /// cross-file targets), convert the editor cursor to a byte offset, find
    /// the resolution whose `use_span` covers it, return a `Location` for the
    /// resolved `def_span`. Falls back to `{param}` slot resolution against
    /// the enclosing decl's parameter list. Returns `null` (None) when
    /// nothing matches or when the resolution kind is `Stdlib`
    /// (per §10.D).
    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> JsonRpcResult<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri.clone();
        let pos = params.text_document_position_params.position;

        let text = {
            let docs = self.documents.read().await;
            match docs.get(&uri) {
                Some(d) => d.text.clone(),
                None => return Ok(None),
            }
        };
        let enable_effects = *self.enable_effects.read().await;

        // Convert URI to a filesystem path so cross-file imports resolve
        // against the importer's directory. Buffers without a `file://`
        // path (rare — `untitled:` or in-memory) fall back to the URI's
        // path component, which still gives same-file resolutions.
        let buffer_path = uri
            .to_file_path()
            .unwrap_or_else(|_| std::path::PathBuf::from(uri.path()));
        let view = match glyph_core::check_source_with_resolutions_at_path(
            &text,
            0,
            &buffer_path,
            enable_effects,
        ) {
            Some(v) => v,
            None => return Ok(None),
        };

        // 1-indexed for LineIndex; LSP positions are 0-indexed.
        let off = view
            .line_index
            .byte_offset(pos.line.saturating_add(1), pos.character.saturating_add(1));

        // Smallest enclosing resolution. Resolutions are span-disjoint by
        // construction (each reference has exactly one resolution), so the
        // first hit is the right one — no need to sort.
        if let Some(r) = view.resolutions.iter().find(|r| {
            off >= r.use_span.start && off < r.use_span.end
        }) {
            // §10.D: stdlib targets return null. The user sees no jump,
            // which matches "subagent has no .glyph to open."
            if r.kind == ResolutionKind::Stdlib {
                return Ok(None);
            }
            // Cross-file branch: when the def lives in a different file, build
            // the target URI from its on-disk path and read that file just to
            // build a LineIndex for the LSP range conversion.
            let same_file = r.def_file == buffer_path
                || r.def_file.as_os_str().is_empty();
            if !same_file {
                let target_uri = match Url::from_file_path(&r.def_file) {
                    Ok(u) => u,
                    Err(_) => return Ok(None),
                };
                let target_text = match std::fs::read_to_string(&r.def_file) {
                    Ok(s) => s,
                    Err(_) => return Ok(None),
                };
                let target_li = LineIndex::new(&target_text);
                let range = convert::byte_span_to_lsp_range(r.def_span, &target_li);
                return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                    uri: target_uri,
                    range,
                })));
            }
            let range = convert::byte_span_to_lsp_range(r.def_span, &view.line_index);
            return Ok(Some(GotoDefinitionResponse::Scalar(Location { uri, range })));
        }

        // Fallback: cursor inside a `{name}` slot in a flow inline string.
        if let Some(param_span) = resolve_param_slot(&text, &view.ast, off) {
            let range = convert::byte_span_to_lsp_range(param_span, &view.line_index);
            return Ok(Some(GotoDefinitionResponse::Scalar(Location { uri, range })));
        }

        Ok(None)
    }

    /// `textDocument/semanticTokens/full` (M3). Walks the lex stream and
    /// AST via [`glyph_core::semantic_tokens::collect_semantic_tokens`]
    /// and delta-encodes the result into the LSP `data: Vec<u32>` shape.
    ///
    /// Returns `None` if the buffer isn't open (rare — the editor only
    /// asks after a `didOpen`). Empty token list is `Some(SemanticTokens
    /// { data: [] })`, not `None` — `None` would tell the editor "no
    /// semantic tokens available," which suppresses fallback highlighters
    /// in some clients.
    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> JsonRpcResult<Option<SemanticTokensResult>> {
        let uri = params.text_document.uri;
        let text = {
            let docs = self.documents.read().await;
            match docs.get(&uri) {
                Some(d) => d.text.clone(),
                None => return Ok(None),
            }
        };

        let raw = collect_semantic_tokens(&text, 0);
        let data = encode_semantic_tokens(&raw);
        Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data,
        })))
    }
}

/// Delta-encode a sorted list of [`RawSemToken`] into the LSP
/// `data: Vec<SemanticToken>` shape. Tokens MUST be sorted by
/// (line, start) — the collector guarantees that.
fn encode_semantic_tokens(raw: &[RawSemToken]) -> Vec<SemanticToken> {
    let mut out: Vec<SemanticToken> = Vec::with_capacity(raw.len());
    let mut prev_line: u32 = 0;
    let mut prev_start: u32 = 0;
    for t in raw {
        let delta_line = t.line - prev_line;
        let delta_start = if delta_line == 0 {
            t.start - prev_start
        } else {
            t.start
        };
        out.push(SemanticToken {
            delta_line,
            delta_start,
            length: t.length,
            token_type: t.token_type,
            token_modifiers_bitset: t.modifiers,
        });
        prev_line = t.line;
        prev_start = t.start;
    }
    out
}

/// Resolve a cursor inside a `{name}` slot to the enclosing decl's
/// parameter span.
///
/// We don't carry slot byte-spans in the AST (FlowStmt::InlineString is a
/// bare `String`), so this scans the source text directly. Algorithm:
///
/// 1. Find the smallest top-level decl whose span covers the cursor.
/// 2. Within that decl's source slice, run `slot::scan_slots` to get every
///    `{name}` and check whether the cursor offset falls inside any slot.
/// 3. Look up the slot's `name` against the decl's param list — return the
///    matching `Param.span` if found.
///
/// Returns `None` if any step fails. The LSP relays that as "no
/// definition" — which is also what an unresolvable slot already produces
/// via `G::analyze::unknown-param-slot`.
fn resolve_param_slot(
    source: &str,
    ast: &glyph_core::ast::SourceFile,
    cursor: u32,
) -> Option<Span> {
    // Find the smallest enclosing top-level decl. Top-level decls don't
    // nest, so any cover is fine.
    let (decl_span, params) = ast.decls.iter().find_map(|d| match d {
        Decl::Skill(s) if covers(s.span, cursor) => Some((s.span, s.node.params.as_slice())),
        Decl::Block(b) if covers(b.span, cursor) => Some((b.span, b.node.params.as_slice())),
        Decl::ExportBlock(eb) if covers(eb.span, cursor) => {
            Some((eb.span, eb.node.params.as_slice()))
        }
        _ => None,
    })?;

    // Restrict the slot scan to flow inline strings inside the enclosing
    // decl. `{name}` is only legal in instruction-bearing strings (per
    // `design/values-and-names.md` §No Interpolation), so we walk just the
    // flow lists rather than scanning the entire source slice.
    let flow_strings = collect_flow_inline_strings(ast, decl_span);
    let body_start = decl_span.start as usize;
    let body_end = decl_span.end as usize;
    let body_text = source.get(body_start..body_end)?;

    for s in &flow_strings {
        // Each flow inline string appears verbatim in the source, surrounded
        // by quotes. Find the literal substring inside the decl's source
        // slice. `find` is O(n) per string; the decl is small, so fine.
        // (We can't get a perfect match for strings whose value contains
        // escaped characters; the parser cooks the value but the source
        // keeps escapes. For MVP escapes don't carry slots, and `{name}`
        // tokens never include escape sequences, so this is acceptable.)
        let mut search_from = 0usize;
        while let Some(rel) = body_text[search_from..].find(s.as_str()) {
            let abs_start = body_start + search_from + rel;
            let abs_end = abs_start + s.len();
            if cursor as usize >= abs_start && (cursor as usize) < abs_end {
                // Cursor is somewhere in this string. Walk slots.
                let inner_offset = cursor as usize - abs_start;
                for slot in glyph_core::slot::scan_slots(s) {
                    if inner_offset >= slot.start_in_content
                        && inner_offset < slot.end_in_content
                    {
                        // Look up param by name.
                        if let Some(p) = params.iter().find(|p| p.name == slot.name) {
                            return Some(p.span);
                        }
                        return None;
                    }
                }
                return None;
            }
            search_from += rel + s.len();
        }
    }

    None
}

fn covers(span: Span, off: u32) -> bool {
    off >= span.start && off < span.end
}

/// Collect every `FlowStmt::InlineString` content reachable inside the
/// decl whose span is `decl_span`.
fn collect_flow_inline_strings(
    ast: &glyph_core::ast::SourceFile,
    decl_span: Span,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for d in &ast.decls {
        match d {
            Decl::Skill(s) if s.span == decl_span => {
                gather_strings(&s.node.flow, &mut out);
            }
            Decl::Block(b) if b.span == decl_span => {
                gather_strings(&b.node.flow, &mut out);
            }
            Decl::ExportBlock(eb) if eb.span == decl_span => {
                // Slice 4 doesn't lower export-block flow; `flow_strings`
                // captures the inline-string content the parser saw.
                out.extend(eb.node.flow_strings.iter().cloned());
            }
            _ => {}
        }
    }
    out
}

fn gather_strings(stmts: &[FlowStmt], out: &mut Vec<String>) {
    for s in stmts {
        match s {
            FlowStmt::InlineString(t) => out.push(t.clone()),
            FlowStmt::Branch { then_body, elif_branches, else_body, .. } => {
                gather_strings(then_body, out);
                for elif in elif_branches {
                    gather_strings(&elif.body, out);
                }
                if let Some(eb) = else_body {
                    gather_strings(eb, out);
                }
            }
            FlowStmt::Return(ReturnExpr::Inline(t)) => out.push(t.clone()),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cursor inside `{scope}` resolves to the skill's `scope` parameter.
    #[test]
    fn param_slot_resolves_to_param() {
        let src = r#"skill main(scope = ".")
    description: "main."
    flow:
        "Inspect {scope} for issues."
"#;
        let view = glyph_core::check_source_with_resolutions(src, 0, "test.glyph", false)
            .expect("parse");
        // Find the byte offset of the `s` inside `{scope}`.
        let off = src.find("{scope}").unwrap() as u32 + 1; // inside the braces
        let span = resolve_param_slot(src, &view.ast, off).expect("should resolve");
        // The param span should cover the parameter declaration in the
        // header. The param name is `scope` — verify the span starts at
        // or near `scope = ".",` in the source.
        let head = &src[span.start as usize..span.end as usize];
        assert!(head.contains("scope"), "param span should cover `scope`, got: {:?}", head);
    }

    /// Cursor inside a slot whose name is not a known parameter returns None.
    #[test]
    fn unknown_param_slot_returns_none() {
        let src = r#"skill main()
    description: "main."
    flow:
        "Use {missing} here."
"#;
        let view = glyph_core::check_source_with_resolutions(src, 0, "test.glyph", false)
            .expect("parse");
        let off = src.find("{missing}").unwrap() as u32 + 1;
        assert!(resolve_param_slot(src, &view.ast, off).is_none());
    }

    /// Cursor outside any slot returns None.
    #[test]
    fn cursor_outside_slot_returns_none() {
        let src = r#"skill main(scope = ".")
    description: "main."
    flow:
        "Inspect things."
"#;
        let view = glyph_core::check_source_with_resolutions(src, 0, "test.glyph", false)
            .expect("parse");
        let off = src.find("Inspect").unwrap() as u32 + 2;
        assert!(resolve_param_slot(src, &view.ast, off).is_none());
    }

    // -----------------------------------------------------------------
    // Resolution-table tests for go-to-definition (M2).
    //
    // We exercise the resolution table directly rather than the full
    // LSP handler — the handler wraps the same machinery plus an LSP
    // range conversion. The conversion is covered by `convert::tests`.
    // -----------------------------------------------------------------

    /// Cursor on a same-file `block` call jumps to the `block` declaration.
    #[test]
    fn block_call_resolves_same_file() {
        let src = r#"skill main()
    description: "main."
    flow:
        validate_plan()

block validate_plan()
    "Check the plan."
"#;
        let view = glyph_core::check_source_with_resolutions(src, 0, "test.glyph", false)
            .expect("parse");
        // Cursor inside the `validate_plan` call-site (first occurrence —
        // the second is the `block` declaration's name token).
        let call_offset = src.find("validate_plan()").unwrap() as u32 + 2;
        let r = view
            .resolutions
            .iter()
            .find(|r| call_offset >= r.use_span.start && call_offset < r.use_span.end)
            .expect("expected a resolution under the cursor");
        assert_eq!(r.kind, ResolutionKind::Block);
        let def_text = &src[r.def_span.start as usize..r.def_span.start as usize + 5];
        assert_eq!(def_text, "block");
    }

    /// Cursor on a same-file bare-name binding reference jumps to the `const`
    /// declaration. Post-#81 the `text` keyword was removed and consts took
    /// over value bindings; `ResolutionKind::Text` is retained as the unified
    /// label for the resolution kind.
    #[test]
    fn bare_name_resolves_to_text_decl() {
        let src = r#"skill main()
    description: "main."
    require accuracy
    flow:
        "Be careful."

const accuracy = "Be accurate."
"#;
        let view = glyph_core::check_source_with_resolutions(src, 0, "test.glyph", false)
            .expect("parse");
        // Cursor inside `accuracy` after `require`.
        let off = src.find("require accuracy").unwrap() as u32 + "require ".len() as u32 + 1;
        let r = view
            .resolutions
            .iter()
            .find(|r| off >= r.use_span.start && off < r.use_span.end)
            .expect("expected a Text resolution under the cursor");
        assert_eq!(r.kind, ResolutionKind::Text);
        let def_text = &src[r.def_span.start as usize..r.def_span.start as usize + 5];
        assert_eq!(def_text, "const");
    }

    /// Cursor on a stdlib reference returns no jump (`Stdlib` kind, which
    /// the handler maps to `null`).
    #[test]
    fn stdlib_reference_marks_stdlib() {
        let src = r#"import "@glyph/std" { subagent }

skill main()
    description: "main."
    flow:
        subagent()
"#;
        let view = glyph_core::check_source_with_resolutions(src, 0, "test.glyph", false)
            .expect("parse");
        // Cursor inside the `subagent` call-site. Skip the import line.
        let off = src.find("subagent()").unwrap() as u32 + 2;
        let r = view
            .resolutions
            .iter()
            .find(|r| off >= r.use_span.start && off < r.use_span.end)
            .expect("expected a stdlib resolution under the cursor");
        assert_eq!(r.kind, ResolutionKind::Stdlib);
    }

    /// Cursor on whitespace finds no resolution — handler returns `null`.
    #[test]
    fn whitespace_cursor_no_resolution() {
        let src = r#"skill main()
    description: "main."
    flow:
        validate_plan()

block validate_plan()
    "Check the plan."
"#;
        let view = glyph_core::check_source_with_resolutions(src, 0, "test.glyph", false)
            .expect("parse");
        // Cursor on a leading-whitespace position (start of the indented line).
        let off = src.find("    description").unwrap() as u32;
        let hit = view
            .resolutions
            .iter()
            .find(|r| off >= r.use_span.start && off < r.use_span.end);
        assert!(hit.is_none(), "no resolution should cover whitespace, got: {:?}", hit);
        // And the param-slot fallback should also yield None.
        assert!(resolve_param_slot(src, &view.ast, off).is_none());
    }

    /// Cursor on a cross-file imported call resolves to the `export block`
    /// declaration in the dependency file. Verifies that the path-aware
    /// entry point follows imports and emits a Resolution whose `def_file`
    /// points at the imported file.
    #[test]
    fn cross_file_import_resolves_to_dep_file() {
        // Lay out the corpus in a tempdir so `resolve_import_path` can
        // canonicalize the dependency path.
        let dir = tempfile::tempdir().expect("tempdir");
        let dep_path = dir.path().join("repo_tools.glyph");
        let dep_src = "export block inspect_repo(scope = \".\")\n    description: \"Inspect.\"\n    flow:\n        \"Examine.\"\n";
        std::fs::write(&dep_path, dep_src).expect("write dep");

        let importer_path = dir.path().join("fix_bug.glyph");
        let importer_src = "import \"./repo_tools.glyph\" { inspect_repo }\n\nskill fix_bug(scope = \".\")\n    description: \"Fix.\"\n    flow:\n        inspect_repo(scope)\n";
        std::fs::write(&importer_path, importer_src).expect("write importer");

        let view = glyph_core::check_source_with_resolutions_at_path(
            importer_src,
            0,
            &importer_path,
            false,
        )
        .expect("parse");

        // Cursor inside the `inspect_repo` call-site (in the flow block).
        let call_idx = importer_src.find("inspect_repo(scope)").unwrap();
        let off = call_idx as u32 + 2;
        let r = view
            .resolutions
            .iter()
            .find(|r| off >= r.use_span.start && off < r.use_span.end)
            .expect("expected a cross-file resolution under the cursor");
        assert_eq!(r.kind, ResolutionKind::ExportBlock);
        // The def_file should be the dependency file path (canonicalized).
        let dep_canon = dep_path.canonicalize().expect("canon dep");
        assert_eq!(r.def_file, dep_canon, "def_file should point at the dep");
        // The def_span should cover the `export block` declaration in the
        // dependency file.
        let def_text = &dep_src[r.def_span.start as usize..(r.def_span.start as usize + 6)];
        assert_eq!(def_text, "export");
    }

    /// M3 cross-file diagnostics: when an importer's clean buffer triggers
    /// the analyzer to surface a diagnostic in an imported dep, the LSP
    /// must be able to map the dep's path → URI for the publish.
    ///
    /// This exercises the key wiring step in `lint_and_publish`: each
    /// `(path, bag)` entry where path != buffer_path becomes an
    /// `Url::from_file_path(path)` publish.
    #[test]
    fn cross_file_diagnostic_attributable_to_dep_uri() {
        let dir = tempfile::tempdir().expect("tempdir");
        let dep_path = dir.path().join("dep.glyph");
        // Dep has its own diagnostic (`require ghost` → undefined-name).
        let dep_text = "\
export const alpha = \"alpha.\"

skill dep_skill()
    description: \"dep skill.\"
    require ghost
    flow:
        \"hello\"
";
        std::fs::write(&dep_path, dep_text).expect("write dep");

        let importer_path = dir.path().join("main.glyph");
        let importer_src = "\
import \"./dep.glyph\" { alpha }

skill main()
    description: \"main.\"
    require alpha
    flow:
        \"hello\"
";
        std::fs::write(&importer_path, importer_src).expect("write importer");

        let bags =
            glyph_core::check_source_with_imports(importer_src, 0, &importer_path, false);

        let canon_dep = dep_path.canonicalize().expect("canon dep");
        let canon_importer = importer_path.canonicalize().expect("canon importer");

        // Both files have entries.
        assert!(bags.contains_key(&canon_importer));
        assert!(bags.contains_key(&canon_dep));

        // Importer is clean; dep carries the undefined-name.
        let importer_bag = bags.get(&canon_importer).unwrap();
        assert!(
            importer_bag
                .iter()
                .all(|d| !matches!(d.classification, glyph_core::diagnostic::Classification::Error)),
            "importer should be clean"
        );
        let dep_bag = bags.get(&canon_dep).unwrap();
        assert!(
            dep_bag
                .iter()
                .any(|d| d.id.starts_with("G::analyze::undefined")),
            "dep should carry undefined-name"
        );

        // Wiring step: dep canonical path round-trips through Url::from_file_path
        // back to a usable LSP URI (the same conversion lint_and_publish does).
        let dep_uri = Url::from_file_path(&canon_dep).expect("dep path → file:// URI");
        assert_eq!(dep_uri.scheme(), "file");
        assert_eq!(
            dep_uri.to_file_path().expect("URI → path round-trip"),
            canon_dep,
            "URI ↔ path round-trip should match"
        );
    }

    // -----------------------------------------------------------------
    // Semantic tokens (M3 Phase B) — LSP delta-encoder pinning tests.
    //
    // The collector is unit-tested exhaustively in `glyph_core::semantic_tokens`.
    // Here we pin the LSP-side delta encoding: tokens on the same line use
    // delta_start = (col - prev_col); a new line resets delta_start to the
    // absolute column.
    // -----------------------------------------------------------------

    #[test]
    fn semantic_tokens_delta_encoding_same_line() {
        // Two tokens on line 0: cols 0 ("skill") and 6 ("main").
        let raw = vec![
            RawSemToken {
                line: 0,
                start: 0,
                length: 5,
                token_type: SemTokenType::Keyword as u32,
                modifiers: 0,
            },
            RawSemToken {
                line: 0,
                start: 6,
                length: 4,
                token_type: SemTokenType::Function as u32,
                modifiers: SemTokenModifier::DECLARATION,
            },
        ];
        let encoded = encode_semantic_tokens(&raw);
        assert_eq!(encoded.len(), 2);
        assert_eq!(encoded[0].delta_line, 0);
        assert_eq!(encoded[0].delta_start, 0);
        assert_eq!(encoded[0].length, 5);
        assert_eq!(encoded[1].delta_line, 0);
        // Same line ⇒ delta_start is the column difference.
        assert_eq!(encoded[1].delta_start, 6);
        assert_eq!(encoded[1].length, 4);
        assert_eq!(encoded[1].token_modifiers_bitset, SemTokenModifier::DECLARATION);
    }

    #[test]
    fn semantic_tokens_delta_encoding_new_line_resets_delta_start() {
        let raw = vec![
            RawSemToken {
                line: 0,
                start: 6,
                length: 4,
                token_type: SemTokenType::Function as u32,
                modifiers: 0,
            },
            RawSemToken {
                line: 2,
                start: 4,
                length: 11,
                token_type: SemTokenType::Keyword as u32,
                modifiers: 0,
            },
        ];
        let encoded = encode_semantic_tokens(&raw);
        // delta_line = 2; new line ⇒ delta_start is the absolute column.
        assert_eq!(encoded[1].delta_line, 2);
        assert_eq!(encoded[1].delta_start, 4);
    }

    #[test]
    fn semantic_tokens_collector_through_lsp_encoding_smoke() {
        // End-to-end: collector → encoder for a small representative source.
        // This pins that the two halves combine correctly (sort + delta).
        let src = "skill main()\n    description: \"d\"\n    flow:\n        \"hi\"\n";
        let raw = collect_semantic_tokens(src, 0);
        assert!(!raw.is_empty(), "non-trivial source should produce tokens");
        let encoded = encode_semantic_tokens(&raw);
        assert_eq!(encoded.len(), raw.len());
        // First token's delta_line must equal the first raw token's line
        // (since prev_line starts at 0).
        assert_eq!(encoded[0].delta_line, raw[0].line);
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

