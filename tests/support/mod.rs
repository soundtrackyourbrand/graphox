#![allow(dead_code)]

use apollo_compiler::Schema;
use futures_util::StreamExt;
use graphox::Backend;
use std::sync::Arc;
pub type LspBackend = Arc<Backend>;
use graphox::{CodegenConfig, DocumentLanguage, DocumentState};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tempfile::TempDir;

pub mod baseline;
pub mod builders;
pub mod fixtures;
pub mod lsp;

pub use builders::ProjectConfigBuilder;
use tower_lsp::LspService;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::Url;
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range};
// serde_json is used via explicit fully-qualified calls in this module.
use graphox::Config;
use tokio::time::Duration;
use tower_lsp::lsp_types::{
    CompletionResponse, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DocumentDiagnosticParams, DocumentDiagnosticReportResult,
    InitializeParams, TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem,
    VersionedTextDocumentIdentifier,
};
use tower_service::Service;

// Additional LSP helpers for tests --------------------------------------------------

/// Create an LSP service with the given `config`, run the initialize sequence, and
/// return the service and backend. This consolidates the common pattern of
/// `LspService::new(...)` + initialize used in many tests.
pub async fn create_initialized_lsp_service(
    config: Config,
) -> (LspService<LspBackend>, tokio::task::JoinHandle<()>) {
    let (mut service, socket) = LspService::new(|client| Backend::new(client, config));
    let handle = tokio::spawn(async move {
        socket.for_each(|_| std::future::ready(())).await;
    });
    lsp_initialize_sequence(&mut service).await;
    (service, handle)
}

/// Create an LSP service similarly to `LspService::new(...)` and spawn a
/// background task that consumes the socket. This mirrors the common test
/// pattern where the socket is not inspected (the original code often called
/// `let (mut service, _) = LspService::new(...)`). Use this helper so tests
/// can avoid referencing `LspService::new` directly when we want a uniform
/// place for creation behavior.
pub fn create_service(config: Config) -> (LspService<LspBackend>, tokio::task::JoinHandle<()>) {
    let (service, socket) = LspService::new(|client| Backend::new(client, config));
    let handle = tokio::spawn(async move {
        socket.for_each(|_| std::future::ready(())).await;
    });
    (service, handle)
}

/// Variant of `create_initialized_lsp_service` that returns the LSP socket stream
/// instead of spawning a consumer task. Use this in tests that need to inspect or
/// consume raw JSON-RPC messages (for example progress notifications or custom
/// server->client messages). The returned stream yields `tower_lsp::jsonrpc::Incoming`.
pub async fn create_initialized_lsp_service_with_socket(
    config: Config,
) -> (
    LspService<LspBackend>,
    tokio_stream::wrappers::UnboundedReceiverStream<serde_json::Value>,
) {
    use tokio::sync::mpsc::unbounded_channel;
    use tokio_stream::wrappers::UnboundedReceiverStream;

    let (mut service, socket) = LspService::new(|client| Backend::new(client, config));

    let (tx, rx) = unbounded_channel();
    // Spawn a task that forwards raw Incoming messages into a serde_json::Value
    tokio::spawn(async move {
        let mut socket = socket;
        while let Some(incoming) = socket.next().await {
            let method = incoming.method().to_string();
            let params = incoming.params().cloned();
            let json = serde_json::json!({"method": method, "params": params});
            let _ = tx.send(json);
        }
    });

    lsp_initialize_sequence(&mut service).await;

    (service, UnboundedReceiverStream::new(rx))
}

/// Create an LSP service and return the raw socket stream without running
/// the initialize sequence. Use this when tests need to send a custom
/// Initialize request (e.g., to advertise capabilities) and also inspect
/// the raw incoming messages via the socket.
pub fn create_lsp_service_with_socket(
    config: Config,
) -> (
    LspService<LspBackend>,
    tokio_stream::wrappers::UnboundedReceiverStream<serde_json::Value>,
) {
    // Return a JSON stream instead of the internal `Incoming` type so tests can
    // inspect messages without referencing non-public tower_lsp types.
    use tokio::sync::mpsc::unbounded_channel;
    use tokio_stream::wrappers::UnboundedReceiverStream;

    let (service, mut socket) = LspService::new(|client| Backend::new(client, config));
    let (tx, rx) = unbounded_channel();
    tokio::spawn(async move {
        while let Some(incoming) = socket.next().await {
            let method = incoming.method().to_string();
            let params = incoming.params().cloned();
            let json = serde_json::json!({"method": method, "params": params});
            let _ = tx.send(json);
        }
    });

    (service, UnboundedReceiverStream::new(rx))
}

/// Create an initialized LSP service and immediately open `uri` with `language_id` and `text`.
/// Returns the service and backend. Version uses `version = 1` for the opened document.
pub async fn create_service_and_open(
    config: Config,
    uri: Url,
    language_id: &str,
    text: &str,
) -> (LspService<LspBackend>, tokio::task::JoinHandle<()>) {
    let (mut service, socket) = LspService::new(|client| Backend::new(client, config));
    let handle = tokio::spawn(async move {
        socket.for_each(|_| std::future::ready(())).await;
    });
    lsp_initialize_sequence(&mut service).await;
    lsp_did_open(&mut service, uri, language_id, 1, text).await;
    (service, handle)
}

/// Variant of `create_service_and_open` that returns the LSP socket stream for
/// tests that need to observe raw messages. The caller is responsible for
/// consuming the socket stream (e.g. by iterating `messages.next().await`).
pub async fn create_service_and_open_with_socket(
    config: Config,
    uri: Url,
    language_id: &str,
    text: &str,
) -> (
    LspService<LspBackend>,
    tokio_stream::wrappers::UnboundedReceiverStream<serde_json::Value>,
) {
    use tokio::sync::mpsc::unbounded_channel;
    use tokio_stream::wrappers::UnboundedReceiverStream;

    let (mut service, mut socket) = LspService::new(|client| Backend::new(client, config));
    lsp_initialize_sequence(&mut service).await;
    lsp_did_open(&mut service, uri, language_id, 1, text).await;

    let (tx, rx) = unbounded_channel();
    tokio::spawn(async move {
        while let Some(incoming) = socket.next().await {
            let method = incoming.method().to_string();
            let params = incoming.params().cloned();
            let json = serde_json::json!({"method": method, "params": params});
            let _ = tx.send(json);
        }
    });

    (service, UnboundedReceiverStream::new(rx))
}

/// Request completion items at `position` and return the parsed `CompletionResponse`.
pub async fn lsp_request_completion(
    service: &mut LspService<LspBackend>,
    uri: Url,
    position: Position,
) -> Option<tower_lsp::lsp_types::CompletionResponse> {
    use tower_lsp::lsp_types::{
        CompletionParams, TextDocumentIdentifier, TextDocumentPositionParams,
    };

    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: None,
    };

    let request = Request::build("textDocument/completion")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();

    let response = match Service::call(service, request).await {
        Ok(res) => res.unwrap(),
        Err(e) => panic!("LSP request failed: {:?}", e),
    };
    serde_json::from_value(response.result().unwrap().clone()).unwrap()
}

/// Request hover at `position` and return the parsed `Hover` (if any).
pub async fn lsp_request_hover(
    service: &mut LspService<LspBackend>,
    uri: Url,
    position: Position,
) -> Option<tower_lsp::lsp_types::Hover> {
    use tower_lsp::lsp_types::{HoverParams, TextDocumentIdentifier, TextDocumentPositionParams};

    let params = HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position,
        },
        work_done_progress_params: Default::default(),
    };

    let request = Request::build("textDocument/hover")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();

    let response = match Service::call(service, request).await {
        Ok(res) => res.unwrap(),
        Err(e) => panic!("LSP request failed: {}", e),
    };
    serde_json::from_value(response.result().unwrap().clone()).unwrap()
}

/// Apply a single LSP `TextEdit` to `original` and return the resulting text.
/// This mirrors the manual edit application logic used across completion tests.
pub fn apply_text_edit(original: &str, edit: &tower_lsp::lsp_types::TextEdit) -> String {
    let lines: Vec<&str> = original.split('\n').collect();
    let start_line = edit.range.start.line as usize;
    let start_char = edit.range.start.character as usize;
    let end_line = edit.range.end.line as usize;
    let end_char = edit.range.end.character as usize;

    // Build the resulting content
    let mut new_content = String::new();
    for (i, line) in lines.iter().enumerate() {
        if i < start_line {
            new_content.push_str(line);
            new_content.push('\n');
        } else if i == start_line {
            let prefix = &line[..start_char.min(line.len())];
            new_content.push_str(prefix);
            new_content.push_str(&edit.new_text);
            if start_line == end_line {
                let suffix = &line[end_char.min(line.len())..];
                new_content.push_str(suffix);
                if i < lines.len() - 1 {
                    new_content.push('\n');
                }
            } else if i < end_line {
                // skip middle lines until end_line
            }
        } else if i > end_line {
            new_content.push_str(line);
            if i < lines.len() - 1 {
                new_content.push('\n');
            }
        } else if i == end_line {
            let suffix = &line[end_char.min(line.len())..];
            new_content.push_str(suffix);
            if i < lines.len() - 1 {
                new_content.push('\n');
            }
        }
    }

    // If the edit extends past EOF (rare in tests), ensure new text is appended
    if new_content.is_empty() {
        new_content.push_str(&edit.new_text);
    }

    new_content
}

/// Find a completion item by label in a completion response array.
pub fn find_completion_by_label<'a>(
    items: &'a [tower_lsp::lsp_types::CompletionItem],
    label: &str,
) -> Option<&'a tower_lsp::lsp_types::CompletionItem> {
    items.iter().find(|i| i.label == label)
}

/// Find a code action by title in a code action response.
pub fn find_code_action_by_title<'a>(
    items: &'a [tower_lsp::lsp_types::CodeActionOrCommand],
    title: &str,
) -> Option<&'a tower_lsp::lsp_types::CodeAction> {
    for item in items {
        if let tower_lsp::lsp_types::CodeActionOrCommand::CodeAction(ca) = item
            && ca.title == title
        {
            return Some(ca);
        }
    }
    None
}

/// Create a temporary project with a single schema file and return the tempdir and a
/// `Config` that points at it. `include_pattern` should match test files (e.g. "**/*.graphql").
pub fn make_temp_project_with_schema(
    schema_text: &str,
    include_pattern: &str,
) -> (TempDir, Config) {
    use graphox::config::{GlobPattern, ProjectConfig, SchemaSource};

    let dir = TempDir::new().expect("failed to create tempdir");
    let schema_path = dir.path().join("schema.graphql");
    fs::write(&schema_path, schema_text).expect("write schema");
    fs::write(dir.path().join("package.json"), "{}").expect("write package.json");

    let config = Config::new_test(
        dir.path().to_path_buf(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single(include_pattern.to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_enable_schema_cache(false)
    .with_lsp_automatic_codegen(false);

    (dir, config)
}

/// Request code actions for given `params` and return the parsed response.
pub async fn lsp_request_code_actions(
    service: &mut LspService<LspBackend>,
    params: tower_lsp::lsp_types::CodeActionParams,
    id: i64,
) -> Option<tower_lsp::lsp_types::CodeActionResponse> {
    let request = Request::build("textDocument/codeAction")
        .id(id)
        .params(serde_json::to_value(&params).unwrap())
        .finish();

    let response = match tokio::time::timeout(
        std::time::Duration::from_millis(20),
        Service::call(service, request),
    )
    .await
    {
        Ok(Ok(res)) => res.unwrap(),
        e => panic!("LSP request failed: {:?}", e),
    };
    serde_json::from_value(response.result().unwrap().clone()).unwrap()
}

/// Normalize a `CompletionResponse` into a flat `Vec<CompletionItem>`.
/// Accepts an `Option<CompletionResponse>` as returned by `lsp_request_completion`.
pub fn completion_items_array(
    response: &Option<tower_lsp::lsp_types::CompletionResponse>,
) -> Vec<tower_lsp::lsp_types::CompletionItem> {
    match response {
        Some(CompletionResponse::Array(items)) => items.clone(),
        Some(CompletionResponse::List(list)) => list.items.clone(),
        None => vec![],
    }
}

/// Send an LSP notification.
pub async fn lsp_send_notification<P>(
    service: &mut LspService<LspBackend>,
    method: &'static str,
    params: &P,
) where
    P: serde::Serialize,
{
    let request = Request::build(method)
        .params(serde_json::to_value(params).unwrap())
        .finish();

    let _ = Service::call(service, request).await.unwrap();
}

/// Generic helper to send an arbitrary LSP request and parse the typed response.
/// Useful for tests that need to call non-trivial methods without writing
/// the Request building/parsing boilerplate each time.
pub async fn lsp_request_typed<T, P>(
    service: &mut LspService<LspBackend>,
    method: &'static str,
    params: &P,
) -> T
where
    T: serde::de::DeserializeOwned,
    P: serde::Serialize,
{
    let request = Request::build(method)
        .id(1)
        .params(serde_json::to_value(params).unwrap())
        .finish();

    let response = match Service::call(service, request).await {
        Ok(res) => res.unwrap(),
        Err(e) => panic!("LSP request failed: {}", e),
    };

    if let Some(err) = response.error() {
        panic!("LSP Error response for {}: {:?}", method, err);
    }

    serde_json::from_value(response.result().unwrap().clone()).unwrap()
}

/// Find a code action or command by its title. Returns a cloned `CodeActionOrCommand`
/// if either a `CodeAction` or `Command` has a matching title.
pub fn find_code_action_or_command_by_title(
    items: &[tower_lsp::lsp_types::CodeActionOrCommand],
    title: &str,
) -> Option<tower_lsp::lsp_types::CodeActionOrCommand> {
    use tower_lsp::lsp_types::CodeActionOrCommand;

    for item in items {
        match item {
            CodeActionOrCommand::CodeAction(ca) if ca.title == title => return Some(item.clone()),
            CodeActionOrCommand::Command(cmd) if cmd.title == title => return Some(item.clone()),
            _ => {}
        }
    }
    None
}

/// Write a file inside `dir` at `rel` and return a `Url` to the file. Convenience
/// wrapper used by tests that build temporary project layouts.
pub fn write_project_file(dir: &TempDir, rel: &str, contents: &str) -> Url {
    write_project_file_at(dir.path(), rel, contents)
}

/// Variant of `write_project_file` that takes a `Path` instead of a `TempDir`.
pub fn write_project_file_at(dir: &Path, rel: &str, contents: &str) -> Url {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create dirs");
    }
    fs::write(&path, contents).expect("write file");
    let canonical = std::fs::canonicalize(path).expect("canonicalize");
    Url::from_file_path(canonical).expect("from_file_path")
}

/// Open multiple files via `didOpen` notifications. Accepts a vector of owned
/// tuples to avoid lifetime complications in tests: (uri, language_id, version, text).
pub async fn lsp_open_files_owned(
    service: &mut LspService<LspBackend>,
    files: Vec<(Url, String, i32, String)>,
) {
    for (uri, language_id, version, text) in files {
        lsp_did_open(service, uri, &language_id, version, &text).await;
    }
}

/// Compute the cursor position after inserting a snippet-like `insert_text` that
/// contains a `$0` placeholder. Returns Some(position) if `$0` found, otherwise None.
pub fn snippet_cursor_after_insert(insert_text: &str, start: Position) -> Option<Position> {
    if !insert_text.contains("$0") {
        return None;
    }

    let before = insert_text.split("$0").next().unwrap_or("");
    let lines_before = before.matches('\n').count() as u32;
    let last_line = before.lines().last().unwrap_or("");

    let expected_line = start.line + lines_before;
    let expected_char = if lines_before > 0 {
        last_line.chars().count() as u32
    } else {
        start.character + last_line.chars().count() as u32
    };

    Some(pos(expected_line, expected_char))
}

/// Apply a completion item to `original` at `position`. Returns (new_text, optional_new_position).
/// The returned position is Some when the completion contained a snippet with `$0`.
pub fn apply_completion_item(
    original: &str,
    position: Position,
    item: &tower_lsp::lsp_types::CompletionItem,
) -> (String, Option<Position>) {
    use tower_lsp::lsp_types::CompletionTextEdit;

    let mut new_text = original.to_string();
    let mut snippet_text = None;
    let mut start_pos = position;

    // If item has a TextEdit, apply it
    if let Some(te) = &item.text_edit {
        match te {
            CompletionTextEdit::Edit(text_edit) => {
                new_text = apply_text_edit(original, text_edit);
                snippet_text = Some(text_edit.new_text.clone());
                start_pos = text_edit.range.start;
            }
            CompletionTextEdit::InsertAndReplace(_) => {
                // Not handled yet, fallback to original behavior for now
            }
        }
    } else {
        // Otherwise, use insert_text or label at position
        let insert_text = item.insert_text.as_deref().unwrap_or(&item.label);
        snippet_text = Some(insert_text.to_string());

        let mut lines: Vec<String> = original.split('\n').map(|s| s.to_string()).collect();
        let line_idx = position.line as usize;
        if line_idx >= lines.len() {
            let mut s = original.to_string();
            s.push_str(insert_text);
            new_text = s;
        } else {
            let line = &lines[line_idx];
            let char_idx = position.character as usize;
            let prefix = &line[..char_idx.min(line.len())];
            let suffix = &line[char_idx.min(line.len())..];
            lines[line_idx] = format!("{}{}{}", prefix, insert_text, suffix);
            new_text = lines.join("\n");
        }
    }

    // If we have a snippet, compute the new position
    // Special case: if text_edit didn't have $0 but insert_text did, use insert_text for snippet calculation
    let effective_snippet = if let Some(ref s) = snippet_text
        && s.contains("$0")
    {
        Some(s.clone())
    } else {
        item.insert_text
            .as_ref()
            .filter(|s| s.contains("$0"))
            .cloned()
    };

    if let Some(snippet) = effective_snippet {
        let before = snippet.split("$0").next().unwrap_or("");
        let lines_before = before.matches('\n').count() as u32;
        let last_line = before.lines().last().unwrap_or("");

        let expected_line = start_pos.line + lines_before;
        let expected_char = if lines_before > 0 {
            last_line.chars().count() as u32
        } else {
            start_pos.character + last_line.chars().count() as u32
        };

        // Also make sure to remove $0 from the final text if it was applied via insert_text logic
        // (apply_text_edit doesn't know about $0)
        let final_content = new_text.replace("$0", "");
        return (final_content, Some(pos(expected_line, expected_char)));
    }

    (new_text, None)
}

/// Helper to find the cursor position marked by '|' in a string and return the
/// string with the marker removed.
pub fn with_cursor(text: &str) -> (String, Position) {
    let cursor_pos = text.find('|').expect("No cursor marker '|' found in text");
    let before = &text[..cursor_pos];
    let after = &text[cursor_pos + 1..];

    let line = before.matches('\n').count();
    let col = if let Some(last_line) = before.lines().last() {
        last_line.chars().count()
    } else {
        before.chars().count()
    };

    (format!("{}{}", before, after), pos(line as u32, col as u32))
}

/// Helper to find multiple cursor positions marked by '|' in a string.
/// Returns the string with markers removed and a Vec of Positions.
pub fn with_cursors(text: &str) -> (String, Vec<Position>) {
    let mut positions = Vec::new();
    let mut clean_text = String::new();
    let mut current_line = 0;
    let mut current_col = 0;

    for c in text.chars() {
        if c == '|' {
            positions.push(pos(current_line, current_col));
        } else {
            clean_text.push(c);
            if c == '\n' {
                current_line += 1;
                current_col = 0;
            } else {
                current_col += 1;
            }
        }
    }

    (clean_text, positions)
}

/// Convenience constructor for LSP positions.
pub fn pos(line: u32, col: u32) -> Position {
    Position::new(line, col)
}

/// Convenience constructor for LSP ranges.
pub fn range(sline: u32, scol: u32, eline: u32, ecol: u32) -> Range {
    Range::new(Position::new(sline, scol), Position::new(eline, ecol))
}

/// Find a diagnostic by its string code.
pub fn find_diag_by_code<'a>(diags: &'a [Diagnostic], code: &str) -> Option<&'a Diagnostic> {
    diags.iter().find(|d| match &d.code {
        Some(NumberOrString::String(s)) => s == code,
        _ => false,
    })
}

/// Find a diagnostic by the exact message.
pub fn find_diag_by_message<'a>(diags: &'a [Diagnostic], message: &str) -> Option<&'a Diagnostic> {
    diags.iter().find(|d| d.message == message)
}

/// Compute a Position for the last occurrence of `token` in `text`.
pub fn pos_for_token(doc: &DocumentState, text: &str, token: &str) -> Position {
    let start_byte = text
        .rfind(token)
        .unwrap_or_else(|| panic!("Token '{}' not found in text", token));
    doc.byte_to_position(start_byte)
}

/// Compute a Position for the nth occurrence of `token` in `text` (0-indexed).
pub fn pos_for_token_at_index(
    doc: &DocumentState,
    text: &str,
    token: &str,
    index: usize,
) -> Position {
    let mut current_pos = 0;
    let mut count = 0;
    while let Some(start_byte) = text[current_pos..].find(token) {
        let absolute_start = current_pos + start_byte;
        if count == index {
            return doc.byte_to_position(absolute_start);
        }
        current_pos = absolute_start + token.len();
        count += 1;
    }
    panic!("Token '{}' at index {} not found in text", token, index);
}

/// Compute a Range for the last occurrence of `token` in `text` using the document's
/// `byte_to_position` helper. Panics if the token isn't found.
pub fn range_for_token(doc: &DocumentState, text: &str, token: &str) -> Range {
    let start_byte = text
        .rfind(token)
        .unwrap_or_else(|| panic!("Token '{}' not found in text", token));
    let end_byte = start_byte + token.len();
    Range {
        start: doc.byte_to_position(start_byte),
        end: doc.byte_to_position(end_byte),
    }
}

/// Compute a Range for the nth occurrence of `token` in `text` (0-indexed).
/// Panics if the token isn't found at the given index.
pub fn range_for_token_at_index(
    doc: &DocumentState,
    text: &str,
    token: &str,
    index: usize,
) -> Range {
    let mut current_pos = 0;
    let mut count = 0;
    while let Some(start_byte) = text[current_pos..].find(token) {
        let absolute_start = current_pos + start_byte;
        if count == index {
            let end_byte = absolute_start + token.len();
            return Range {
                start: doc.byte_to_position(absolute_start),
                end: doc.byte_to_position(end_byte),
            };
        }
        current_pos = absolute_start + token.len();
        count += 1;
    }
    panic!("Token '{}' at index {} not found in text", token, index);
}

// --- Assertion helpers -------------------------------------------------

/// Wait for a condition to become true, polling at regular intervals.
/// Default timeout is 2 seconds, with 10ms polling interval.
pub async fn wait_for_condition<F>(condition: F) -> bool
where
    F: Fn() -> bool,
{
    wait_for_condition_with_timeout(condition, Duration::from_secs(2)).await
}

/// Wait for a condition to become true, with a custom timeout.
pub async fn wait_for_condition_with_timeout<F>(condition: F, timeout: Duration) -> bool
where
    F: Fn() -> bool,
{
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if condition() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    condition()
}

/// Assert that `diags` contains a diagnostic whose message equals `expected_message` exactly.
/// Returns a reference to the found Diagnostic for further inspection.
pub fn assert_diag_message_equals<'a>(
    diags: &'a [Diagnostic],
    expected_message: &str,
) -> &'a Diagnostic {
    match diags.iter().find(|d| d.message == expected_message) {
        Some(d) => d,
        None => panic!(
            "Expected diagnostic with exact message '{}', got: {:#?}",
            expected_message, diags
        ),
    }
}

/// Assert there are no diagnostics.
pub fn assert_no_diagnostics(diags: &[Diagnostic]) {
    if !diags.is_empty() {
        panic!("Expected no diagnostics, but found: {:#?}", diags);
    }
}

/// Assert that a diagnostic's range equals `expected`.
pub fn assert_diag_range_equals(diag: &Diagnostic, expected: &Range) {
    if diag.range != *expected {
        panic!(
            "Diagnostic range mismatch. expected: {:?}, got: {:?}\nDiagnostic: {:#?}",
            expected, diag.range, diag
        );
    }
}

/// Assert that diagnostics contains exactly one diagnostic containing the expected message.
/// Returns the matching diagnostic for further assertions.
///
/// # Arguments
/// * `diags` - The list of diagnostics to search
/// * `expected_message` - A substring to match in the diagnostic message
///
/// # Panics
/// If zero or more than one diagnostic contains the expected message.
pub fn assert_diagnostic_with_message<'a>(
    diags: &'a [Diagnostic],
    expected_message: &str,
) -> &'a Diagnostic {
    let matching: Vec<_> = diags
        .iter()
        .filter(|d| d.message.contains(expected_message))
        .collect();

    assert!(
        matching.len() == 1,
        "Expected exactly 1 diagnostic containing '{}', but found {} diagnostics containing it and {} total. \
         All diagnostics: {:#?}",
        expected_message,
        matching.len(),
        diags.len(),
        diags
    );

    matching[0]
}

/// Assert diagnostic severity.
pub fn assert_diagnostic_severity(diag: &Diagnostic, expected: DiagnosticSeverity) {
    assert_eq!(
        diag.severity,
        Some(expected),
        "Expected severity {:?}, got {:?}. Full diagnostic: {:#?}",
        expected,
        diag.severity,
        diag
    );
}

/// Assert there are exactly `expected` diagnostics, with a helpful error message.
pub fn assert_diagnostics_count(diags: &[Diagnostic], expected: usize) {
    assert_eq!(
        diags.len(),
        expected,
        "Expected {} diagnostic(s), got {}. Diagnostics: {:#?}",
        expected,
        diags.len(),
        diags
    );
}

/// Small temp workspace helper for tests that need filesystem-backed files.
pub struct TestWorkspace {
    tmp: TempDir,
}

impl TestWorkspace {
    pub fn new() -> Self {
        Self {
            tmp: TempDir::new().expect("failed to create tempdir"),
        }
    }

    pub fn root(&self) -> &Path {
        self.tmp.path()
    }

    /// Write a file relative to the workspace root and return its path.
    pub fn write_file(&self, rel: &str, contents: &str) -> PathBuf {
        let path = self.tmp.path().join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create dirs");
        }
        fs::write(&path, contents).expect("write file");
        path
    }

    pub fn uri_for(&self, rel: &str) -> Url {
        let path = std::fs::canonicalize(self.tmp.path().join(rel)).expect("canonicalize");
        Url::from_file_path(path).expect("from_file_path")
    }

    /// Copy a directory recursively to the workspace root.
    pub fn copy_from(&self, src: impl AsRef<Path>) {
        copy_dir_all(src.as_ref(), self.tmp.path()).expect("failed to copy directory");
    }
}

/// Copy a directory recursively from `src` to `dst`.
pub fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> std::io::Result<()> {
    fs::create_dir_all(&dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}

static SCHEMA: OnceLock<Schema> = OnceLock::new();
static VALID_SCHEMA: OnceLock<apollo_compiler::validation::Valid<Schema>> = OnceLock::new();

/// Return a parsed Schema loaded from tests/fixtures/simple_schema.graphql.
pub fn get_schema() -> &'static Schema {
    SCHEMA.get_or_init(|| {
        let schema_content = std::fs::read_to_string("tests/fixtures/simple_schema.graphql")
            .expect("Failed to read schema file");
        Schema::parse(&schema_content, "schema.graphql").expect("Failed to parse schema")
    })
}

/// Return a validated Schema, cached in a OnceLock.
pub fn get_valid_schema() -> &'static apollo_compiler::validation::Valid<Schema> {
    VALID_SCHEMA.get_or_init(|| {
        get_schema()
            .clone()
            .validate()
            .expect("Schema validation failed")
    })
}

/// Create a DocumentState for given uri and text. Mirrors previous helpers used in tests.
pub fn create_doc(uri_str: &str, text: &str) -> DocumentState {
    let uri = Url::parse(uri_str).unwrap();
    let language = DocumentLanguage::from_uri(&uri);

    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&language.get_parser_language())
        .unwrap();

    DocumentState::new(
        uri,
        text,
        &mut parser,
        tower_lsp::lsp_types::PositionEncodingKind::UTF16,
    )
}

/// Start an LSP service paired with a Backend created from `config`.
/// Returns the service and the backend instance (the same as LspService::new()).
/// Run the standard initialize sequence (initialize + initialized) against the service.
pub async fn lsp_initialize_sequence(service: &mut LspService<LspBackend>) {
    // initialize
    let init_req = Request::build("initialize")
        .params(serde_json::to_value(InitializeParams::default()).unwrap())
        .id(0)
        .finish();
    let _ = Service::call(service, init_req).await.unwrap().unwrap();

    // initialized notification
    let init_notif = Request::build("initialized")
        .params(serde_json::json!({}))
        .finish();
    let _ = Service::call(service, init_notif).await.unwrap();

    // Wait for workspace scan to complete by checking workspace_loaded flag
    let backend = service.inner();
    let start = std::time::Instant::now();
    while !backend
        .workspace_loaded
        .load(std::sync::atomic::Ordering::SeqCst)
    {
        if start.elapsed().as_secs() > 10 {
            println!("Wait for workspace_loaded TIMEOUT at {:?}", start.elapsed());
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Simulate opening a text document via didOpen notification.
pub async fn lsp_did_open(
    service: &mut LspService<LspBackend>,
    uri: Url,
    language_id: &str,
    version: i32,
    text: &str,
) {
    let params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri,
            language_id: language_id.to_string(),
            version,
            text: text.to_string(),
        },
    };
    let req = Request::build("textDocument/didOpen")
        .params(serde_json::to_value(params).unwrap())
        .finish();
    let _ = Service::call(service, req).await.unwrap();
}

/// Simulate closing a text document via didClose notification.
pub async fn lsp_did_close(service: &mut LspService<LspBackend>, uri: Url) {
    let params = DidCloseTextDocumentParams {
        text_document: TextDocumentIdentifier { uri },
    };
    let req = Request::build("textDocument/didClose")
        .params(serde_json::to_value(params).unwrap())
        .finish();
    let _ = Service::call(service, req).await.unwrap();
}

/// Simulate updating a text document via didChange notification.
pub async fn lsp_did_change(
    service: &mut LspService<LspBackend>,
    uri: Url,
    version: i32,
    text: &str,
) {
    let params = DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier { uri, version },
        content_changes: vec![TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: text.to_string(),
        }],
    };
    let req = Request::build("textDocument/didChange")
        .params(serde_json::to_value(params).unwrap())
        .finish();
    let _ = Service::call(service, req).await.unwrap();
}

/// Request diagnostics for a document and return the parsed `DocumentDiagnosticReportResult`.
pub async fn lsp_request_diagnostics(
    service: &mut LspService<LspBackend>,
    uri: Url,
) -> DocumentDiagnosticReportResult {
    let params = DocumentDiagnosticParams {
        text_document: tower_lsp::lsp_types::TextDocumentIdentifier { uri },
        identifier: None,
        previous_result_id: None,
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };
    let request = Request::build("textDocument/diagnostic")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = match Service::call(service, request).await {
        Ok(res) => res.unwrap(),
        Err(e) => panic!("LSP request failed: {}", e),
    };
    serde_json::from_value(response.result().unwrap().clone())
        .expect("failed to parse diagnostic result")
}

// Convenience wrapper around `LspService::new` was intentionally removed —
// call `LspService::new(|client| Backend::new(client, config))` inline in tests.

// =============================================================================
// Performance Test Helpers
// =============================================================================

/// Serializes performance tests that measure live heap *or* retain a large
/// workspace for the duration of the test. [`measure_allocated_bytes`] reads a
/// process-wide counter, so a heavy test allocating concurrently would pollute
/// another test's before/after delta. Every such test takes this lock so their
/// measurement windows never overlap, making the deltas deterministic.
pub static PERF_MEMORY_MUTEX: once_cell::sync::Lazy<tokio::sync::Mutex<()>> =
    once_cell::sync::Lazy::new(|| tokio::sync::Mutex::new(()));

/// Measure current resident set size (RSS) in bytes (platform-specific).
/// Takes multiple samples and returns the minimum to filter out temporary spikes.
/// Live heap bytes (allocated minus freed) seen by [`TrackingAllocator`].
/// Reads as zero in binaries that do not install the tracking allocator.
pub static LIVE_HEAP_BYTES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// A `System`-backed global allocator that records the number of live heap
/// bytes in [`LIVE_HEAP_BYTES`]. A test binary opts in by declaring it as its
/// `#[global_allocator]`. Counting live bytes directly is deterministic, unlike
/// process RSS, which also reflects allocator pool retention, freed-but-unreturned
/// pages, thread stacks and mapped files.
pub struct TrackingAllocator;

unsafe impl std::alloc::GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        let ptr = unsafe { std::alloc::System.alloc(layout) };
        if !ptr.is_null() {
            LIVE_HEAP_BYTES.fetch_add(layout.size(), std::sync::atomic::Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        unsafe { std::alloc::System.dealloc(ptr, layout) };
        LIVE_HEAP_BYTES.fetch_sub(layout.size(), std::sync::atomic::Ordering::Relaxed);
    }

    unsafe fn alloc_zeroed(&self, layout: std::alloc::Layout) -> *mut u8 {
        let ptr = unsafe { std::alloc::System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            LIVE_HEAP_BYTES.fetch_add(layout.size(), std::sync::atomic::Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: std::alloc::Layout, new_size: usize) -> *mut u8 {
        let new_ptr = unsafe { std::alloc::System.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            if new_size >= layout.size() {
                LIVE_HEAP_BYTES.fetch_add(
                    new_size - layout.size(),
                    std::sync::atomic::Ordering::Relaxed,
                );
            } else {
                LIVE_HEAP_BYTES.fetch_sub(
                    layout.size() - new_size,
                    std::sync::atomic::Ordering::Relaxed,
                );
            }
        }
        new_ptr
    }
}

/// Current live heap bytes, as tracked by [`TrackingAllocator`]. Deterministic
/// alternative to [`measure_memory_usage`]; only meaningful in binaries that
/// install the tracking allocator as their global allocator.
pub fn measure_allocated_bytes() -> usize {
    LIVE_HEAP_BYTES.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn measure_memory_usage() -> usize {
    let mut min_rss = usize::MAX;

    for _ in 0..3 {
        let current_rss = {
            #[cfg(target_os = "macos")]
            {
                use std::mem;
                let mut info: libc::mach_task_basic_info = unsafe { mem::zeroed() };
                let mut count = libc::MACH_TASK_BASIC_INFO_COUNT;
                #[allow(deprecated)]
                let ret = unsafe {
                    libc::task_info(
                        libc::mach_task_self(),
                        libc::MACH_TASK_BASIC_INFO,
                        &mut info as *mut libc::mach_task_basic_info as *mut libc::integer_t,
                        &mut count,
                    )
                };
                if ret == libc::KERN_SUCCESS {
                    info.resident_size as usize
                } else {
                    0
                }
            }
            #[cfg(target_os = "linux")]
            {
                if let Ok(content) = std::fs::read_to_string("/proc/self/statm") {
                    let parts: Vec<&str> = content.split_whitespace().collect();
                    if parts.len() >= 2 {
                        if let Ok(pages) = parts[1].parse::<usize>() {
                            let ps = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
                            if ps > 0 {
                                let page_size = ps as usize;
                                pages.checked_mul(page_size).unwrap_or(0)
                            } else {
                                0
                            }
                        } else {
                            0
                        }
                    } else {
                        0
                    }
                } else {
                    0
                }
            }
            #[cfg(target_os = "windows")]
            {
                0
            }
        };

        if current_rss > 0 && current_rss < min_rss {
            min_rss = current_rss;
        }

        // Small pause between samples
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    if min_rss == usize::MAX { 0 } else { min_rss }
}

/// Wait for a file to exist on disk. Synchronous.
pub fn wait_for_file(path: &Path, timeout: Duration) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if path.exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

/// Wait for a file to exist on disk and optionally contain specific text. Asynchronous.
pub async fn wait_for_file_async(
    path: &Path,
    timeout: Duration,
    expected_content: Option<&str>,
) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if path.exists() {
            if let Some(expected) = expected_content {
                if let Ok(content) = fs::read_to_string(path)
                    && content.contains(expected)
                {
                    return true;
                }
            } else {
                return true;
            }
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    false
}

/// Create a complex schema A with N types (E-commerce themed).
pub fn create_complex_schema_a(num_types: usize) -> String {
    let mut schema =
        String::from("directive @key(fields: String!) on OBJECT | INTERFACE\n\ntype Query {\n");
    schema.push_str("    user(id: ID!): User\n");
    schema.push_str("    product(id: ID!): Product\n");
    schema.push_str("    order(id: ID!): Order\n");
    schema.push_str("}\n\n");

    schema.push_str("type Mutation {\n");
    schema.push_str("    updateUser(id: ID!): User\n");
    schema.push_str("    updateProduct(id: ID!): Product\n");
    schema.push_str("    updateOrder(id: ID!): Order\n");
    schema.push_str("}\n\n");

    schema.push_str("type Subscription {\n");
    schema.push_str("    UserChanged: User\n");
    schema.push_str("    ProductChanged: Product\n");
    schema.push_str("    OrderChanged: Order\n");
    schema.push_str("}\n\n");

    schema.push_str("type User @key(fields: \"id\") {\n");
    schema.push_str("    id: ID!\n");
    schema.push_str("    username: String!\n");
    schema.push_str("    profile: Profile!\n");
    schema.push_str("}\n\n");

    schema.push_str("type Profile {\n");
    schema.push_str("    firstName: String\n");
    schema.push_str("    lastName: String\n");
    schema.push_str("    avatarUrl: String\n");
    schema.push_str("}\n\n");

    schema.push_str("type Product @key(fields: \"id\") {\n");
    schema.push_str("    id: ID!\n");
    schema.push_str("    sku: String!\n");
    schema.push_str("    price: Int!\n");
    schema.push_str("    category: Category!\n");
    schema.push_str("}\n\n");

    schema.push_str("type Category {\n");
    schema.push_str("    id: ID!\n");
    schema.push_str("    name: String!\n");
    schema.push_str("}\n\n");

    schema.push_str("type Order @key(fields: \"id\") {\n");
    schema.push_str("    id: ID!\n");
    schema.push_str("    items: [OrderItem!]!\n");
    schema.push_str("    total: Int!\n");
    schema.push_str("    customer: User!\n");
    schema.push_str("}\n\n");

    schema.push_str("type OrderItem {\n");
    schema.push_str("    product: Product!\n");
    schema.push_str("    quantity: Int!\n");
    schema.push_str("}\n\n");

    for i in 0..(num_types.saturating_sub(7)) {
        schema.push_str(&format!("type ExtraTypeA{} {{\n", i));
        schema.push_str("    id: ID!\n");
        schema.push_str("    field: String\n");
        schema.push_str("}\n\n");
    }

    schema
}

/// Create a complex schema B with N types (Content themed).
pub fn create_complex_schema_b(num_types: usize) -> String {
    let mut schema =
        String::from("directive @key(fields: String!) on OBJECT | INTERFACE\n\ntype Query {\n");
    schema.push_str("    article(id: ID!): Article\n");
    schema.push_str("    media(id: ID!): Media\n");
    schema.push_str("    analytics(id: ID!): Analytics\n");
    schema.push_str("}\n\n");

    schema.push_str("type Mutation {\n");
    schema.push_str("    updateArticle(id: ID!): Article\n");
    schema.push_str("    updateMedia(id: ID!): Media\n");
    schema.push_str("}\n\n");

    schema.push_str("type Subscription {\n");
    schema.push_str("    ArticleChanged: Article\n");
    schema.push_str("    MediaChanged: Media\n");
    schema.push_str("}\n\n");

    schema.push_str("type Article @key(fields: \"id\") {\n");
    schema.push_str("    id: ID!\n");
    schema.push_str("    title: String!\n");
    schema.push_str("    body: String!\n");
    schema.push_str("    author: Author!\n");
    schema.push_str("    comments: [Comment!]!\n");
    schema.push_str("}\n\n");

    schema.push_str("type Author @key(fields: \"id\") {\n");
    schema.push_str("    id: ID!\n");
    schema.push_str("    name: String!\n");
    schema.push_str("    articles: [Article!]!\n");
    schema.push_str("}\n\n");

    schema.push_str("type Comment {\n");
    schema.push_str("    id: ID!\n");
    schema.push_str("    text: String!\n");
    schema.push_str("    article: Article!\n");
    schema.push_str("}\n\n");

    schema.push_str("type Media @key(fields: \"id\") {\n");
    schema.push_str("    id: ID!\n");
    schema.push_str("    url: String!\n");
    schema.push_str("    type: MediaType!\n");
    schema.push_str("}\n\n");

    schema.push_str("enum MediaType {\n");
    schema.push_str("    IMAGE\n");
    schema.push_str("    VIDEO\n");
    schema.push_str("    AUDIO\n");
    schema.push_str("}\n\n");

    schema.push_str("type Analytics {\n");
    schema.push_str("    views: Int!\n");
    schema.push_str("    likes: Int!\n");
    schema.push_str("}\n\n");

    for i in 0..(num_types.saturating_sub(7)) {
        schema.push_str(&format!("type ExtraTypeB{} {{\n", i));
        schema.push_str("    id: ID!\n");
        schema.push_str("    field: String\n");
        schema.push_str("}\n\n");
    }

    schema
}

/// Create a project with fragments and operations.
pub fn create_project_with_fragments(
    project_dir: &Path,
    schema_type: &str, // "A", "B", or "both"
    project_idx: usize,
    _total_projects: usize,
) {
    fs::create_dir_all(project_dir).unwrap();

    let (type_name, field_name) = match schema_type {
        "A" => ("User", "username"),
        "B" => ("Article", "title"),
        _ => ("User", "username"), // Default to A for "both" in fragments
    };

    // 100 fragments (30 public, 70 private)
    let mut fragments_content = String::new();
    for i in 0..100 {
        let is_public = i < 30;
        let frag_name = if is_public {
            format!("PublicFrag_{}_{}", project_idx, i)
        } else {
            format!("PrivateFrag_{}_{}", project_idx, i)
        };

        fragments_content.push_str(&format!(
            "fragment {} on {} {{ {} }}\n",
            frag_name, type_name, field_name
        ));
    }
    fs::write(project_dir.join("fragments.graphql"), fragments_content).unwrap();

    // 300 operations (100 queries, 100 mutations, 100 subscriptions)
    // 80% are TS/TSX
    for i in 0..300 {
        let op_type = match i % 3 {
            0 => "query",
            1 => "mutation",
            _ => "subscription",
        };

        let op_name = format!("Op_{}_{}_{}", project_idx, op_type, i);
        let gql_content = match op_type {
            "query" => format!("query {} {{ {} {{ id }} }}", op_name, field_name),
            "mutation" => format!(
                "mutation {} {{ update{}(id: \"1\") {{ id }} }}",
                op_name, type_name
            ),
            _ => format!(
                "subscription {} {{ {}Changed {{ id }} }}",
                op_name, type_name
            ),
        };

        let is_ts = (i % 10) < 8; // 80%
        let ext = if is_ts {
            if i % 2 == 0 { "ts" } else { "tsx" }
        } else {
            "graphql"
        };

        let filename = format!("op_{}.{}", i, ext);
        let content = if is_ts {
            format!("const {} = gql`\n  {}\n`;", op_name, gql_content)
        } else {
            gql_content
        };

        fs::write(project_dir.join(filename), content).unwrap();
    }
}

/// Time a closure and return (duration, result).
pub fn timed<T>(f: impl FnOnce() -> T) -> (std::time::Duration, T) {
    let start = std::time::Instant::now();
    let result = f();
    (start.elapsed(), result)
}

/// Create a large schema with N types.
pub fn create_large_schema(num_types: usize) -> String {
    let mut schema = String::from("type Query { ");
    for i in 0..num_types {
        schema.push_str(&format!("item{}: Item{} ", i, i));
    }
    schema.push_str("}\n");

    for i in 0..num_types {
        schema.push_str(&format!("type Item{} {{ id: ID! name: String }}\n", i));
    }
    schema
}

/// Create N fragment definitions.
pub fn create_many_fragments(num_fragments: usize) -> String {
    let mut fragments = String::new();
    for i in 0..num_fragments {
        fragments.push_str(&format!(
            "fragment Frag{} on Query {{ item{} {{ id }} }}\n",
            i,
            i % 100
        ));
    }
    fragments
}

/// Create a deep fragment chain (N levels deep).
pub fn create_deep_fragment_chain(depth: usize) -> String {
    let mut result = String::new();
    for i in 0..depth {
        result.push_str(&format!("fragment Frag{} on Query {{ ", i));
    }
    result.push_str("id\n");
    for i in (0..depth).rev() {
        result.push_str(&format!("}} ...Frag{}\n", i));
    }
    result
}
