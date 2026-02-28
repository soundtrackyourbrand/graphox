use crate::backend::state::Backend;
use graphox_core::DocumentState;
use graphox_features::hover::DocumentHover;
use graphox_features::shared::doc_utils;

use ahash::AHashMap;
use std::sync::Arc;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{Hover, HoverContents, HoverParams, MarkupContent, MarkupKind};

pub async fn handle_hover(backend: &Backend, params: HoverParams) -> Result<Option<Hover>> {
    let uri = backend.normalize_uri(
        params
            .text_document_position_params
            .text_document
            .uri
            .clone(),
    );
    let position = params.text_document_position_params.position;

    if let Some(doc) = backend.documents.get(&uri).map(|r| r.value().clone()) {
        let schema = backend.get_schema_for_doc(&uri);

        let project_subgraphs = if let Ok(path) = uri.to_file_path()
            && let Ok(config) = backend.config.read()
        {
            let schema_key = config.get_schema_for_path(&path);
            schema_key.and_then(|key| backend.subgraphs.get(&key).map(|r| r.value().clone()))
        } else {
            None
        };
        let doc_ref: &DocumentState = doc.as_ref();
        if let Some(hover) = doc_ref.get_hover_info(
            position,
            &schema,
            project_subgraphs.as_deref(),
            &backend.documents,
        ) {
            return Ok(Some(hover));
        }

        let symbol_at_pos = doc.get_symbol_at_position(position);

        return backend
            .with_tracing("hover", async move {
                if let Some(symbol_name) = symbol_at_pos {
                    // Collect documents first to avoid holding DashMap locks during processing
                    let doc_arcs: Vec<Arc<DocumentState>> = backend
                        .documents
                        .iter()
                        .map(|e| e.value().clone())
                        .collect();

                    for other_doc in doc_arcs {
                        let is_same_package = graphox_core::utils::paths_match(
                            other_doc.package_root.as_deref(),
                            doc.package_root.as_deref(),
                        );
                        let is_public_fragment = other_doc
                            .fragments()
                            .iter()
                            .any(|f| f.name.as_ref() == symbol_name && f.is_public);

                        if (is_same_package || is_public_fragment)
                            && let Some(info) = other_doc.find_fragment_info(&symbol_name)
                        {
                            let mut value = format!(
                                "```graphql
{}
```",
                                info
                            );

                            let all_fragments = backend.get_all_fragments_info();
                            let mut variable_types_cache = AHashMap::default();
                            let requirements = backend.get_fragment_requirements(
                                &symbol_name,
                                &schema,
                                doc.package_root.as_ref(),
                                &all_fragments,
                                &mut variable_types_cache,
                            );
                            if !requirements.is_empty() {
                                value.push_str(
                                    "

**Requires Variables:**
",
                                );
                                for (var, ty) in requirements {
                                    value.push_str(&format!(
                                        "- `${}`: `{}`
",
                                        var, ty
                                    ));
                                }
                            }

                            if let Some(desc) =
                                doc_utils::find_description(&other_doc, &symbol_name)
                            {
                                value.push_str(
                                    "

---
",
                                );
                                value.push_str(&desc);
                            }

                            if !is_same_package && let Ok(other_p) = other_doc.uri.to_file_path() {
                                let config = backend.config.read().unwrap();
                                if let Some(proj) = config.get_project_for_path(&other_p)
                                    && let Some(import) = proj.import()
                                {
                                    value.push_str(
                                        "

---
",
                                    );
                                    value.push_str(&format!("Import: `{}`", import));
                                }
                            }

                            return Ok(Some(Hover {
                                contents: HoverContents::Markup(MarkupContent {
                                    kind: MarkupKind::Markdown,
                                    value,
                                }),
                                range: None,
                            }));
                        }
                    }
                }
                Ok(None)
            })
            .await;
    }

    Ok(None)
}
