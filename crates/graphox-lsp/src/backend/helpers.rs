use ahash::AHashSet;
use graphox_core::Config;
use graphox_core::document::OperationDef;
use graphox_core::types::OperationNamesMap;
use std::sync::Arc;
use std::time::Instant;
use tower_lsp::Client;
use tower_lsp::lsp_types::*;

/// Normalize a file URI by resolving it to canonical path
pub fn normalize_uri(uri: Url) -> Url {
    graphox_core::utils::normalize_uri(uri)
}

pub fn named_operation_names(operations: &[OperationDef]) -> Arc<[Arc<str>]> {
    operations
        .iter()
        .filter_map(|operation| operation.name.clone())
        .collect()
}

pub fn update_operation_name_index(
    operation_names: &OperationNamesMap,
    config: &Config,
    uri: &Url,
    old_operation_names: Option<&[Arc<str>]>,
    new_operations: &[OperationDef],
) -> AHashSet<Arc<str>> {
    let mut affected_operation_names = AHashSet::default();
    let old_operation_names: AHashSet<Arc<str>> = old_operation_names
        .into_iter()
        .flat_map(|names| names.iter().cloned())
        .collect();

    for name in &old_operation_names {
        let mut remove_entry = false;
        if let Some(mut entry) = operation_names.get_mut(name) {
            entry.value_mut().retain(|(_, op_uri)| op_uri != uri);
            remove_entry = entry.value().is_empty();
        }
        if remove_entry {
            operation_names.remove(name);
        }
        affected_operation_names.insert(name.clone());
    }

    let Ok(path) = uri.to_file_path() else {
        return affected_operation_names;
    };
    let Some(schema_key) = config.get_schema_for_path(&path) else {
        return affected_operation_names;
    };

    let project_key = config
        .get_project_for_path(&path)
        .map(|project| project.include().as_key())
        .unwrap_or(schema_key);
    let project_key_arc: Arc<str> = project_key.into();

    for operation in new_operations {
        if let Some(name) = &operation.name {
            affected_operation_names.insert(name.clone());
            operation_names
                .entry(name.clone())
                .or_default()
                .push((project_key_arc.clone(), uri.clone()));
        }
    }

    affected_operation_names
}

/// Execute an async operation with tracing and timeout support
pub async fn with_tracing<T, Fut>(
    client: &Client,
    name: &str,
    timeout_ms: u64,
    tracing_config: Option<(bool, u64)>,
    fut: Fut,
) -> tower_lsp::jsonrpc::Result<Option<T>>
where
    Fut: std::future::Future<Output = tower_lsp::jsonrpc::Result<Option<T>>>,
{
    let start = Instant::now();

    // Apply timeout
    let res = match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), fut).await {
        Ok(result) => result,
        Err(_) => {
            let elapsed = start.elapsed();
            client
                .log_message(
                    MessageType::ERROR,
                    format!(
                        "LSP Request '{}' exceeded timeout of {}ms (took {}ms) - returning empty response",
                        name,
                        timeout_ms,
                        elapsed.as_millis()
                    ),
                )
                .await;
            // Return Ok(None) for timed out requests
            Ok(None)
        }
    };

    // Log if tracing is enabled and threshold exceeded
    if let Some((enabled, threshold_ms)) = tracing_config
        && enabled
    {
        let elapsed = start.elapsed();
        if elapsed.as_millis() >= threshold_ms as u128 {
            client
                .log_message(
                    MessageType::INFO,
                    format!("LSP Request '{}' took {}ms", name, elapsed.as_millis()),
                )
                .await;
        }
    }

    res
}

#[cfg(test)]
mod tests {
    use super::*;
    use dashmap::DashMap;
    use graphox_core::config::{GlobPattern, ProjectConfig, SchemaSource};
    use graphox_core::types::OperationNamesMap;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    #[ntest::timeout(3000)]
    fn test_normalize_uri_preserves_valid_uri() {
        let uri = Url::parse("file:///tmp/test.graphql").unwrap();
        let normalized = normalize_uri(uri.clone());
        assert!(normalized.scheme() == "file");
    }

    #[test]
    fn update_operation_name_index_replaces_only_target_uri_entries() {
        let temp_dir = tempdir().unwrap();
        let base_dir = temp_dir.path().to_path_buf();
        fs::write(
            base_dir.join("schema.graphql"),
            "type Query { viewer: String }",
        )
        .unwrap();
        fs::write(
            base_dir.join("query.graphql"),
            "query SharedQuery { viewer }",
        )
        .unwrap();
        fs::write(
            base_dir.join("other.graphql"),
            "query SharedQuery { viewer }",
        )
        .unwrap();

        let config = Config::new_test(
            base_dir.clone(),
            vec![
                ProjectConfig::default()
                    .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                    .with_include(GlobPattern::Single("**/*.graphql".to_string())),
            ],
        );

        let uri = Url::from_file_path(base_dir.join("query.graphql")).unwrap();
        let other_uri = Url::from_file_path(base_dir.join("other.graphql")).unwrap();
        let operation_names: OperationNamesMap =
            Arc::new(DashMap::with_hasher(ahash::RandomState::default()));
        operation_names.insert(
            Arc::from("SharedQuery"),
            vec![
                (Arc::from("**/*.graphql"), uri.clone()),
                (Arc::from("**/*.graphql"), other_uri.clone()),
            ],
        );
        operation_names.insert(
            Arc::from("OldQuery"),
            vec![(Arc::from("**/*.graphql"), uri.clone())],
        );

        let old_operation_names: Arc<[Arc<str>]> =
            vec![Arc::from("SharedQuery"), Arc::from("OldQuery")].into();
        let new_operations = vec![
            OperationDef {
                name: Some(Arc::from("SharedQuery")),
                operation_type: Arc::from("query"),
                source_text: Arc::from("query SharedQuery { viewer }"),
            },
            OperationDef {
                name: Some(Arc::from("NewQuery")),
                operation_type: Arc::from("query"),
                source_text: Arc::from("query NewQuery { viewer }"),
            },
        ];

        let affected = update_operation_name_index(
            &operation_names,
            &config,
            &uri,
            Some(old_operation_names.as_ref()),
            &new_operations,
        );

        assert!(affected.contains("SharedQuery"));
        assert!(affected.contains("OldQuery"));
        assert!(affected.contains("NewQuery"));

        let shared = operation_names.get("SharedQuery").unwrap();
        assert_eq!(shared.len(), 2);
        assert!(shared.iter().any(|(_, op_uri)| op_uri == &uri));
        assert!(shared.iter().any(|(_, op_uri)| op_uri == &other_uri));

        assert!(operation_names.get("OldQuery").is_none());

        let new_query = operation_names.get("NewQuery").unwrap();
        assert_eq!(new_query.len(), 1);
        assert_eq!(new_query[0].1, uri);
    }
}
