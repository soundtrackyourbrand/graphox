use crate::completion::constants::{OPERATION_TYPE_KEYWORDS, SCHEMA_DEFINITION_KEYWORDS};
use crate::shared::markdown_utils;
use ls_types::{CompletionItem, CompletionItemKind};

/// Filter keywords by prefix for operation types
fn get_operation_type_completions_for_prefix(prefix: &str) -> Vec<&'static str> {
    if prefix.is_empty() {
        return OPERATION_TYPE_KEYWORDS.to_vec();
    }

    // Find matching keywords based on prefix
    OPERATION_TYPE_KEYWORDS
        .iter()
        .filter(|&&keyword| keyword.starts_with(prefix))
        .copied()
        .collect()
}

/// Filter keywords by prefix for schema definitions
fn get_schema_keyword_completions_for_prefix(_prefix: &str) -> Vec<&'static str> {
    SCHEMA_DEFINITION_KEYWORDS.to_vec()
}

/// Get completions for operation type keywords (query, mutation, subscription)
pub fn get_operation_type_keyword_completions() -> Vec<CompletionItem> {
    get_operation_type_completions_for_prefix("")
        .into_iter()
        .map(|keyword| CompletionItem {
            label: keyword.to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some(markdown_utils::describe_keyword_detail(keyword)),
            ..Default::default()
        })
        .collect()
}

/// Get completions for schema definition keywords with prefix filtering
pub fn get_schema_definition_keyword_completions(prefix: &str) -> Vec<CompletionItem> {
    get_schema_keyword_completions_for_prefix(prefix)
        .into_iter()
        .map(|keyword| CompletionItem {
            label: keyword.to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some(markdown_utils::describe_schema_keyword_detail(keyword)),
            ..Default::default()
        })
        .collect()
}
