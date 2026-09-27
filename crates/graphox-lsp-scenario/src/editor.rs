//! The editor side of document sync, with the request pattern VS Code
//! produces around it: completion on every identifier keystroke (cancelling the
//! previous one), and a burst of document requests once typing pauses.

use crate::lsp::LspClient;
use crate::pull_diagnostics::DiagnosticsPuller;
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

struct Doc {
    path: PathBuf,
    text: String,
    version: i64,
}

pub struct Editor {
    client: Arc<LspClient>,
    docs: HashMap<String, Doc>,
    /// Shared with the diagnostics puller, which re-pulls these on refresh.
    pub open_uris: Arc<Mutex<BTreeSet<String>>>,
    capabilities: Value,
    pub keystroke: Duration,
    pub request_timeout: Duration,
}

pub fn uri_for(path: &Path) -> String {
    graphox_core::utils::path_to_uri(path)
        .map(|u| u.to_string())
        .unwrap_or_else(|| format!("file://{}", path.display()))
}

fn language_id(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("tsx") => "typescriptreact",
        Some("ts" | "mts" | "cts") => "typescript",
        Some("jsx") => "javascriptreact",
        Some("js" | "mjs" | "cjs") => "javascript",
        _ => "graphql",
    }
}

/// Byte offset to an LSP position in UTF-16 code units.
pub fn position_at(text: &str, offset: usize) -> Value {
    let before = &text[..offset];
    let line = before.matches('\n').count();
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let character: usize = before[line_start..].chars().map(char::len_utf16).sum();
    json!({ "line": line, "character": character })
}

/// LSP position in UTF-16 code units to a byte offset.
pub fn offset_at(text: &str, position: &Value) -> usize {
    let line = position.get("line").and_then(Value::as_u64).unwrap_or(0) as usize;
    let character = position
        .get("character")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    let line_start = if line == 0 {
        0
    } else {
        text.match_indices('\n')
            .nth(line - 1)
            .map_or(text.len(), |(i, _)| i + 1)
    };
    let mut units = 0;
    for (i, ch) in text[line_start..].char_indices() {
        if units >= character || ch == '\n' {
            return line_start + i;
        }
        units += ch.len_utf16();
    }
    text.len()
}

impl Editor {
    pub fn new(client: Arc<LspClient>, capabilities: Value) -> Self {
        Self {
            client,
            docs: HashMap::new(),
            open_uris: Default::default(),
            capabilities,
            keystroke: Duration::from_millis(80),
            request_timeout: Duration::from_secs(30),
        }
    }

    fn supports(&self, provider: &str) -> bool {
        self.capabilities
            .get(provider)
            .is_some_and(|v| !v.is_null() && v != &Value::Bool(false))
    }

    pub fn is_open(&self, path: &Path) -> bool {
        self.docs.contains_key(&uri_for(path))
    }

    pub fn text(&self, path: &Path) -> Option<&str> {
        self.docs.get(&uri_for(path)).map(|d| d.text.as_str())
    }

    pub fn open(&mut self, path: &Path) -> std::io::Result<()> {
        let uri = uri_for(path);
        if self.docs.contains_key(&uri) {
            return Ok(());
        }
        let text = std::fs::read_to_string(path)?;
        self.client.notify(
            "textDocument/didOpen",
            json!({ "textDocument": {
                "uri": uri, "languageId": language_id(path), "version": 1, "text": text
            }}),
        );
        self.docs.insert(
            uri.clone(),
            Doc {
                path: path.to_path_buf(),
                text,
                version: 1,
            },
        );
        self.open_uris.lock().unwrap().insert(uri);
        Ok(())
    }

    pub fn close(&mut self, path: &Path) {
        let uri = uri_for(path);
        if self.docs.remove(&uri).is_some() {
            self.open_uris.lock().unwrap().remove(&uri);
            self.client.notify(
                "textDocument/didClose",
                json!({ "textDocument": { "uri": uri } }),
            );
        }
    }

    pub fn open_paths(&self) -> Vec<PathBuf> {
        let mut paths: Vec<_> = self.docs.values().map(|d| d.path.clone()).collect();
        paths.sort();
        paths
    }

    /// Applies edits (byte range, replacement) in one `didChange`. Edits must
    /// not overlap; they are sent last-first so each applies to the text the
    /// previous one produced.
    pub fn change(&mut self, path: &Path, mut edits: Vec<(usize, usize, String)>) {
        let uri = uri_for(path);
        let Some(doc) = self.docs.get_mut(&uri) else {
            return;
        };
        edits.sort_by_key(|e| std::cmp::Reverse(e.0));
        let mut changes = Vec::new();
        for (start, end, text) in edits {
            changes.push(json!({
                "range": { "start": position_at(&doc.text, start), "end": position_at(&doc.text, end) },
                "text": text
            }));
            doc.text.replace_range(start..end, &text);
        }
        doc.version += 1;
        self.client.notify(
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": uri, "version": doc.version },
                "contentChanges": changes
            }),
        );
    }

    /// Types `text` at byte `offset` one character at a time, and returns the
    /// offset just past it.
    pub fn type_text(
        &mut self,
        path: &Path,
        offset: usize,
        text: &str,
        puller: &DiagnosticsPuller,
    ) -> usize {
        let mut offset = offset;
        let mut completion: Option<i64> = None;
        for ch in text.chars() {
            if let Some(id) = completion.take() {
                self.client.cancel(id);
            }
            self.change(path, vec![(offset, offset, ch.to_string())]);
            offset += ch.len_utf8();
            if (ch.is_alphanumeric() || ch == '_') && self.supports("completionProvider") {
                let position = position_at(self.text(path).unwrap_or(""), offset);
                completion = Some(self.client.request_async(
                    "textDocument/completion",
                    json!({
                        "textDocument": { "uri": uri_for(path) },
                        "position": position,
                        "context": { "triggerKind": 1 }
                    }),
                ));
            }
            thread::sleep(self.keystroke);
            if ch.is_whitespace() {
                self.pause(path, offset, puller);
            }
        }
        self.pause(path, offset, puller);
        offset
    }

    /// The requests an editor sends for the active document once typing stops.
    pub fn pause(&mut self, path: &Path, cursor: usize, puller: &DiagnosticsPuller) {
        let uri = uri_for(path);
        let position = position_at(self.text(path).unwrap_or(""), cursor);
        let doc = json!({ "uri": uri });
        puller.pull_document(&uri);
        if self.supports("semanticTokensProvider") {
            self.client.request_async(
                "textDocument/semanticTokens/full",
                json!({ "textDocument": doc }),
            );
        }
        if self.supports("documentSymbolProvider") {
            self.client.request_async(
                "textDocument/documentSymbol",
                json!({ "textDocument": doc }),
            );
        }
        if self.supports("foldingRangeProvider") {
            self.client
                .request_async("textDocument/foldingRange", json!({ "textDocument": doc }));
        }
        if self.supports("codeActionProvider") {
            self.client.request_async(
                "textDocument/codeAction",
                json!({
                    "textDocument": doc,
                    "range": { "start": position, "end": position },
                    "context": { "diagnostics": [], "triggerKind": 2 }
                }),
            );
        }
        if self.supports("documentHighlightProvider") {
            self.client.request_async(
                "textDocument/documentHighlight",
                json!({ "textDocument": doc, "position": position }),
            );
        }
        thread::sleep(Duration::from_millis(300));
    }

    /// Writes the buffer to disk and sends `didSave`, in that order, as an
    /// editor does. The write also reaches the server through the watcher.
    pub fn save(&mut self, path: &Path) -> std::io::Result<()> {
        let uri = uri_for(path);
        let Some(doc) = self.docs.get(&uri) else {
            return Ok(());
        };
        std::fs::write(&doc.path, &doc.text)?;
        let include_text = self
            .capabilities
            .pointer("/textDocumentSync/save/includeText")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let mut params = json!({ "textDocument": { "uri": uri } });
        if include_text {
            params["text"] = Value::String(doc.text.clone());
        }
        self.client.notify("textDocument/didSave", params);
        Ok(())
    }

    /// Reloads open documents whose file changed on disk, the way an editor
    /// reverts clean buffers after a checkout.
    pub fn sync_from_disk(&mut self) {
        for (uri, doc) in self.docs.iter_mut() {
            let Ok(text) = std::fs::read_to_string(&doc.path) else {
                continue; // deleted: the editor keeps the buffer as it was
            };
            if text == doc.text {
                continue;
            }
            doc.version += 1;
            doc.text = text;
            self.client.notify(
                "textDocument/didChange",
                json!({
                    "textDocument": { "uri": uri, "version": doc.version },
                    "contentChanges": [{ "text": doc.text }]
                }),
            );
        }
    }

    pub fn navigate(&mut self, path: &Path, offset: usize) {
        let uri = uri_for(path);
        let position = position_at(self.text(path).unwrap_or(""), offset);
        let at = json!({ "textDocument": { "uri": uri }, "position": position });
        for (provider, method) in [
            ("hoverProvider", "textDocument/hover"),
            ("definitionProvider", "textDocument/definition"),
        ] {
            if self.supports(provider) {
                let _ = self
                    .client
                    .request(method, at.clone(), self.request_timeout);
            }
        }
        if self.supports("referencesProvider") {
            let mut params = at.clone();
            params["context"] = json!({ "includeDeclaration": true });
            let _ = self
                .client
                .request("textDocument/references", params, self.request_timeout);
        }
    }

    /// Renames the symbol at `offset` and applies the result like VS Code does
    /// with its default refactoring auto-save: files that were not open are
    /// opened in the background, edited, saved and closed again.
    pub fn rename(&mut self, path: &Path, offset: usize, new_name: &str) -> Result<usize, String> {
        let uri = uri_for(path);
        let position = position_at(self.text(path).unwrap_or(""), offset);
        let at = json!({ "textDocument": { "uri": uri }, "position": position });
        if self
            .capabilities
            .pointer("/renameProvider/prepareProvider")
            .and_then(Value::as_bool)
            == Some(true)
        {
            self.client.request(
                "textDocument/prepareRename",
                at.clone(),
                self.request_timeout,
            )?;
        }
        let mut params = at;
        params["newName"] = Value::String(new_name.to_string());
        let edit = self
            .client
            .request("textDocument/rename", params, self.request_timeout)?;

        let mut per_file: HashMap<String, Vec<Value>> = HashMap::new();
        if let Some(changes) = edit.get("changes").and_then(Value::as_object) {
            for (uri, edits) in changes {
                per_file
                    .entry(uri.clone())
                    .or_default()
                    .extend(edits.as_array().cloned().unwrap_or_default());
            }
        }
        if let Some(doc_changes) = edit.get("documentChanges").and_then(Value::as_array) {
            for change in doc_changes {
                if let (Some(uri), Some(edits)) = (
                    change.pointer("/textDocument/uri").and_then(Value::as_str),
                    change.get("edits").and_then(Value::as_array),
                ) {
                    per_file
                        .entry(uri.to_string())
                        .or_default()
                        .extend(edits.iter().cloned());
                }
            }
        }

        let mut background = Vec::new();
        let files = per_file.len();
        for (uri, edits) in per_file {
            let parsed: ls_types::Uri = uri.parse().map_err(|_| format!("bad uri {uri}"))?;
            let Some(path) = graphox_core::utils::uri_to_path(&parsed) else {
                continue;
            };
            if !self.is_open(&path) {
                self.open(&path).map_err(|e| e.to_string())?;
                background.push(path.clone());
            }
            let text = self.text(&path).unwrap_or("").to_string();
            let byte_edits = edits
                .iter()
                .filter_map(|e| {
                    let range = e.get("range")?;
                    Some((
                        offset_at(&text, range.get("start")?),
                        offset_at(&text, range.get("end")?),
                        e.get("newText")?.as_str()?.to_string(),
                    ))
                })
                .collect();
            self.change(&path, byte_edits);
            self.save(&path).map_err(|e| e.to_string())?;
        }
        for path in background {
            self.close(&path);
        }
        Ok(files)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_count_utf16_units_and_round_trip() {
        let text = "query Q {\n  émoji🎉 { id }\n}\n";
        let offset = text.find("{ id").unwrap();
        let position = position_at(text, offset);
        // "  émoji🎉 " is 2 + 5 + 2 (surrogate pair) + 1 UTF-16 units.
        assert_eq!(position, json!({ "line": 1, "character": 10 }));
        assert_eq!(offset_at(text, &position), offset);
    }

    #[test]
    fn offsets_past_the_line_end_clamp_to_it() {
        let text = "ab\ncd";
        assert_eq!(offset_at(text, &json!({ "line": 0, "character": 99 })), 2);
        assert_eq!(
            offset_at(text, &json!({ "line": 9, "character": 0 })),
            text.len()
        );
    }
}
