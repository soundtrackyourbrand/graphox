#![allow(dead_code)]

use apollo_compiler::Schema;
use futures_util::StreamExt;
use graphql_rust::Backend as LspBackend;
use graphql_rust::{DocumentLanguage, DocumentState};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tempfile::TempDir;
use tower_lsp::LspService;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::Url;
use tower_lsp::lsp_types::{Diagnostic, NumberOrString, Position, Range};
// serde_json is used via explicit fully-qualified calls in this module.
use graphql_rust::Config;
use tokio::time::Duration;
use tower_lsp::lsp_types::{
    CompletionResponse, DidOpenTextDocumentParams, DocumentDiagnosticParams,
    DocumentDiagnosticReportResult, InitializeParams, TextDocumentItem,
};
use tower_service::Service;

// Additional LSP helpers for tests --------------------------------------------------

/// Create an LSP service with the given `config`, run the initialize sequence, and
/// return the service and backend. This consolidates the common pattern of
/// `LspService::new(...)` + initialize used in many tests.
pub async fn create_initialized_lsp_service(
    config: Config,
) -> (LspService<LspBackend>, tokio::task::JoinHandle<()>) {
    let (mut service, socket) = LspService::new(|client| LspBackend::new(client, config));
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
    let (service, socket) = LspService::new(|client| LspBackend::new(client, config));
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

    let (mut service, socket) = LspService::new(|client| LspBackend::new(client, config));
    lsp_initialize_sequence(&mut service).await;

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

    (service, UnboundedReceiverStream::new(rx))
}

/// Create an LSP service and return the raw socket stream without running
/// the initialize sequence. Use this when tests need to send a custom
/// Initialize request (e.g., to advertise capabilities) and also inspect
/// the raw incoming messages via the socket.
pub fn create_lsp_service_with_socket(
    config: Config,
) -> (LspService<LspBackend>, tokio_stream::wrappers::UnboundedReceiverStream<serde_json::Value>) {
    // Return a JSON stream instead of the internal `Incoming` type so tests can
    // inspect messages without referencing non-public tower_lsp types.
    use tokio::sync::mpsc::unbounded_channel;
    use tokio_stream::wrappers::UnboundedReceiverStream;

    let (service, mut socket) = LspService::new(|client| LspBackend::new(client, config));
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
    let (mut service, socket) = LspService::new(|client| LspBackend::new(client, config));
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

    let (mut service, mut socket) = LspService::new(|client| LspBackend::new(client, config));
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
    use graphql_rust::config::{GlobPattern, ProjectConfig, SchemaSource};

    let dir = TempDir::new().expect("failed to create tempdir");
    let schema_path = dir.path().join("schema.graphql");
    fs::write(&schema_path, schema_text).expect("write schema");
    fs::write(dir.path().join("package.json"), "{}").expect("write package.json");

    let config = Config {
        base_dir: dir.path().to_path_buf(),
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single(include_pattern.to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        enable_schema_cache: Some(true),
        lsp_automatic_codegen: Some(false),
        ..Config::new_empty()
    };

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

    DocumentState::new(uri, text, parser)
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
