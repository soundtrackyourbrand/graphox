use crate::backend::state::Backend;
use graphox_core::document::CompletionContext;
use graphox_core::document::DocumentState;
use graphox_features::completion::DocumentCompletion;

use ahash::AHashMap;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    CompletionItem, CompletionParams, CompletionResponse, CompletionTextEdit, Position,
    PositionEncodingKind, Range,
};

fn line_max_character(doc: &DocumentState, line: u32) -> u32 {
    let last_line = doc.rope.len_lines().saturating_sub(1) as u32;
    let line_idx = line.min(last_line) as usize;
    let mut line_text = doc.rope.line(line_idx).to_string();
    while line_text.ends_with('\n') || line_text.ends_with('\r') {
        line_text.pop();
    }

    if doc.position_encoding == PositionEncodingKind::UTF8 {
        line_text.len() as u32
    } else if doc.position_encoding == PositionEncodingKind::UTF16 {
        line_text.encode_utf16().count() as u32
    } else {
        line_text.chars().count() as u32
    }
}

fn clamp_position(doc: &DocumentState, pos: Position) -> Position {
    let max_line = doc.rope.len_lines().saturating_sub(1) as u32;
    let line = pos.line.min(max_line);
    let max_char = line_max_character(doc, line);
    Position::new(line, pos.character.min(max_char))
}

fn clamp_range(doc: &DocumentState, range: Range) -> Range {
    let start = clamp_position(doc, range.start);
    let mut end = clamp_position(doc, range.end);
    if end.line < start.line || (end.line == start.line && end.character < start.character) {
        end = start;
    }
    Range::new(start, end)
}

fn sanitize_completion_items(doc: &DocumentState, items: &mut [CompletionItem]) {
    for item in items {
        if let Some(text_edit) = &mut item.text_edit {
            match text_edit {
                CompletionTextEdit::Edit(edit) => {
                    edit.range = clamp_range(doc, edit.range);
                }
                CompletionTextEdit::InsertAndReplace(edit) => {
                    edit.insert = clamp_range(doc, edit.insert);
                    edit.replace = clamp_range(doc, edit.replace);
                }
            }
        }

        if let Some(additional_edits) = &mut item.additional_text_edits {
            for edit in additional_edits {
                edit.range = clamp_range(doc, edit.range);
            }
        }
    }
}

pub async fn handle_completion(
    backend: &Backend,
    params: CompletionParams,
) -> Result<Option<CompletionResponse>> {
    backend
        .with_tracing("completion", async move {
            let uri = backend.normalize_uri(params.text_document_position.text_document.uri);
            let position = params.text_document_position.position;

            if let Some(doc) = backend.documents.get(&uri).map(|r| r.value().clone()) {
                let schema = backend.get_schema_for_doc(&uri);
                let all_fragments = backend.get_all_fragments_info();

                // Optimization: Identify completion context first.
                // If we are not in a selection set, we can skip fragments entirely.
                let context = doc.get_completion_context(position, &schema);

                let mut fragments = match context {
                    CompletionContext::SelectionSet(parent_type) => {
                        let mut filtered = backend.get_fragments_for_doc(&doc, &all_fragments);
                        let parent_name = parent_type.name();

                        filtered.retain(|f| {
                            if f.is_type_only {
                                return false;
                            }
                            // Keep fragment if it's on the same type
                            if f.type_condition.as_ref() == parent_name.as_str() {
                                return true;
                            }

                            // Get the fragment's type from schema
                            let frag_type = match schema.types.get(f.type_condition.as_ref()) {
                                Some(t) => t,
                                None => return true, // If type unknown, play it safe and keep it
                            };

                            // Check for intersection between parent_type and frag_type
                            match (&parent_type, frag_type) {
                                // Object and Interface/Object
                                (
                                    apollo_compiler::schema::ExtendedType::Object(obj),
                                    apollo_compiler::schema::ExtendedType::Interface(_),
                                ) => obj
                                    .implements_interfaces
                                    .iter()
                                    .any(|i| i.as_str() == f.type_condition.as_ref()),
                                (
                                    apollo_compiler::schema::ExtendedType::Interface(_),
                                    apollo_compiler::schema::ExtendedType::Object(obj),
                                ) => obj
                                    .implements_interfaces
                                    .iter()
                                    .any(|i| i.as_str() == parent_name.as_str()),

                                // Union cases
                                (
                                    apollo_compiler::schema::ExtendedType::Union(u),
                                    apollo_compiler::schema::ExtendedType::Object(_),
                                ) => u
                                    .members
                                    .iter()
                                    .any(|m| m.as_str() == f.type_condition.as_ref()),
                                (
                                    apollo_compiler::schema::ExtendedType::Object(_),
                                    apollo_compiler::schema::ExtendedType::Union(u),
                                ) => u.members.iter().any(|m| m.as_str() == parent_name.as_str()),

                                // Interface and Interface (intersection if they share implementors)
                                (
                                    apollo_compiler::schema::ExtendedType::Interface(_),
                                    apollo_compiler::schema::ExtendedType::Interface(_),
                                ) => true,

                                // Union and Interface
                                (
                                    apollo_compiler::schema::ExtendedType::Union(u),
                                    apollo_compiler::schema::ExtendedType::Interface(_),
                                ) => u.members.iter().any(|m| {
                                    if let Some(apollo_compiler::schema::ExtendedType::Object(
                                        obj,
                                    )) = schema.types.get(m.as_str())
                                    {
                                        obj.implements_interfaces
                                            .iter()
                                            .any(|i| i.as_str() == f.type_condition.as_ref())
                                    } else {
                                        false
                                    }
                                }),
                                (
                                    apollo_compiler::schema::ExtendedType::Interface(_),
                                    apollo_compiler::schema::ExtendedType::Union(u),
                                ) => u.members.iter().any(|m| {
                                    if let Some(apollo_compiler::schema::ExtendedType::Object(
                                        obj,
                                    )) = schema.types.get(m.as_str())
                                    {
                                        obj.implements_interfaces
                                            .iter()
                                            .any(|i| i.as_str() == parent_name.as_str())
                                    } else {
                                        false
                                    }
                                }),

                                // Union and Union
                                (
                                    apollo_compiler::schema::ExtendedType::Union(u1),
                                    apollo_compiler::schema::ExtendedType::Union(u2),
                                ) => u1.members.iter().any(|m1| {
                                    u2.members.iter().any(|m2| m1.as_str() == m2.as_str())
                                }),

                                _ => false,
                            }
                        });
                        filtered
                    }
                    CompletionContext::OperationDefinition => Vec::new(),
                    CompletionContext::SchemaDefinition => Vec::new(),
                    CompletionContext::FieldAlias => Vec::new(),
                    CompletionContext::DirectiveArguments => Vec::new(),
                    CompletionContext::UnionMembers => Vec::new(),
                    CompletionContext::ImplementsClause => Vec::new(),
                    CompletionContext::VariableDefaultValue => Vec::new(),
                    CompletionContext::ArgumentDefaultValue => Vec::new(),
                    CompletionContext::Other => Vec::new(),
                };

                log::trace!(
                    "completion: fragments for doc {} = {:?}",
                    doc.uri,
                    fragments.iter().map(|f| f.name.clone()).collect::<Vec<_>>()
                );

                let mut variable_types_cache = AHashMap::default();
                for f in &mut fragments {
                    f.requirements = backend.get_fragment_requirements(
                        &f.name,
                        &schema,
                        doc.package_root.as_ref(),
                        &all_fragments,
                        &mut variable_types_cache,
                    );
                }

                let mut items = doc.get_completion_items(position, &schema, fragments);
                sanitize_completion_items(&doc, &mut items);
                log::trace!(
                    "completion: produced items = {:?}",
                    items.iter().map(|i| i.label.clone()).collect::<Vec<_>>()
                );
                return Ok(Some(CompletionResponse::Array(items)));
            }

            Ok(None)
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::{TextEdit, Url};

    #[test]
    fn test_sanitize_completion_items_clamps_out_of_range_positions() {
        let uri = Url::parse("file:///tmp/test.graphql").expect("valid test uri");
        let doc = DocumentState::new_from_thread_local(
            uri,
            "query {\n  users\n}\n",
            PositionEncodingKind::UTF16,
        );

        let mut items = vec![CompletionItem {
            label: "users".to_string(),
            text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                range: Range::new(Position::new(9, 500), Position::new(10, 600)),
                new_text: "users".to_string(),
            })),
            additional_text_edits: Some(vec![TextEdit {
                range: Range::new(Position::new(1, 999), Position::new(1, 1000)),
                new_text: "x".to_string(),
            }]),
            ..Default::default()
        }];

        sanitize_completion_items(&doc, &mut items);

        let item = &items[0];
        let CompletionTextEdit::Edit(main_edit) = item.text_edit.clone().expect("text edit exists")
        else {
            panic!("expected main edit");
        };
        assert!(main_edit.range.start.line <= 3);
        assert!(main_edit.range.end.line <= 3);
        assert!(
            main_edit.range.start.character <= line_max_character(&doc, main_edit.range.start.line)
        );
        assert!(
            main_edit.range.end.character <= line_max_character(&doc, main_edit.range.end.line)
        );

        let additional = item
            .additional_text_edits
            .as_ref()
            .expect("additional edits exist");
        let extra = &additional[0];
        assert!(extra.range.start.character <= line_max_character(&doc, extra.range.start.line));
        assert!(extra.range.end.character <= line_max_character(&doc, extra.range.end.line));
    }
}
