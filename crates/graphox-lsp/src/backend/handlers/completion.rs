use crate::backend::state::Backend;
use graphox_core::document::CompletionContext;
use graphox_features::completion::DocumentCompletion;

use ahash::AHashMap;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{CompletionParams, CompletionResponse};

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

                let items = doc.get_completion_items(position, &schema, fragments);
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
