use crate::support::{
    create_lsp_service_with_socket, lsp_did_open, lsp_request_hover, make_temp_project_with_schema,
    pos, write_project_file,
};
use std::time::Duration;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_position_encoding_negotiation_utf8() {
    let schema = "type Query { emoji: String }";
    let (_dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _messages) = create_lsp_service_with_socket(config);

    // Send initialize with UTF-8 preference
    let mut params = InitializeParams::default();
    params.capabilities.general = Some(GeneralClientCapabilities {
        position_encodings: Some(vec![PositionEncodingKind::UTF8]),
        ..Default::default()
    });

    let init_req = Request::build("initialize")
        .params(serde_json::to_value(params).unwrap())
        .id(1)
        .finish();

    let response = match Service::call(&mut service, init_req).await {
        Ok(res) => res.unwrap(),
        Err(e) => panic!("Initialize failed: {:?}", e),
    };

    let result: InitializeResult =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();
    assert_eq!(
        result.capabilities.position_encoding,
        Some(PositionEncodingKind::UTF8),
        "Server should have negotiated UTF-8"
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_position_encoding_negotiation_utf16() {
    let schema = "type Query { emoji: String }";
    let (_dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _messages) = create_lsp_service_with_socket(config);

    // Send initialize with UTF-16 preference
    let mut params = InitializeParams::default();
    params.capabilities.general = Some(GeneralClientCapabilities {
        position_encodings: Some(vec![PositionEncodingKind::UTF16]),
        ..Default::default()
    });

    let init_req = Request::build("initialize")
        .params(serde_json::to_value(params).unwrap())
        .id(1)
        .finish();

    let response = match Service::call(&mut service, init_req).await {
        Ok(res) => res.unwrap(),
        Err(e) => panic!("Initialize failed: {:?}", e),
    };

    let result: InitializeResult =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();
    assert_eq!(
        result.capabilities.position_encoding,
        Some(PositionEncodingKind::UTF16),
        "Server should have negotiated UTF-16"
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_position_encoding_negotiation_utf32() {
    let schema = "type Query { emoji: String }";
    let (_dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _messages) = create_lsp_service_with_socket(config);

    // Send initialize with UTF-32 preference
    let mut params = InitializeParams::default();
    params.capabilities.general = Some(GeneralClientCapabilities {
        position_encodings: Some(vec![PositionEncodingKind::UTF32]),
        ..Default::default()
    });

    let init_req = Request::build("initialize")
        .params(serde_json::to_value(params).unwrap())
        .id(1)
        .finish();

    let response = match Service::call(&mut service, init_req).await {
        Ok(res) => res.unwrap(),
        Err(e) => panic!("Initialize failed: {:?}", e),
    };

    let result: InitializeResult =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();
    assert_eq!(
        result.capabilities.position_encoding,
        Some(PositionEncodingKind::UTF32),
        "Server should have negotiated UTF-32"
    );
}

async fn wait_for_workspace_scan(backend: &graphox::Backend) {
    let start = std::time::Instant::now();
    while !backend
        .workspace_loaded
        .load(std::sync::atomic::Ordering::SeqCst)
    {
        if start.elapsed().as_secs() > 5 {
            println!("Warning: Timeout waiting for workspace scan in position_encoding test");
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_hover_range_with_utf8() {
    let schema = "type Query { field(arg: String): User } type User { emoji: String }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config.base_dir = std::fs::canonicalize(&config.base_dir).unwrap();
    let (mut service, _messages) = create_lsp_service_with_socket(config);

    // 1. Initialize with UTF-8
    let mut params = InitializeParams::default();
    params.capabilities.general = Some(GeneralClientCapabilities {
        position_encodings: Some(vec![PositionEncodingKind::UTF8]),
        ..Default::default()
    });
    let init_req = Request::build("initialize")
        .params(serde_json::to_value(params).unwrap())
        .id(1)
        .finish();
    let _ = Service::call(&mut service, init_req).await.unwrap();

    // Trigger initialized
    let initialized_notif = Request::build("initialized")
        .params(serde_json::json!({}))
        .finish();
    let _ = Service::call(&mut service, initialized_notif)
        .await
        .unwrap();

    wait_for_workspace_scan(service.inner()).await;

    // 2. Open file with emoji in string argument
    let text = "{ field(arg: \"🚀\") { emoji } }";
    let uri = write_project_file(&dir, "utf8.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    // 3. Request hover at \"emoji\"
    // Sum: 1+1+1+1+1+1+1+1 + 4 + 1+1+1+1+1+1 = 14 + 4 = 18 ? No.
    // Let's re-calculate:
    // { (1) + space (1) + field (5) + ( (1) + arg (3) + : (1) + space (1) + \" (1) + 🚀 (4) + \" (1) + ) (1) + space (1) + { (1) + space (1)
    // = 1 + 1 + 5 + 1 + 3 + 1 + 1 + 1 + 4 + 1 + 1 + 1 + 1 + 1 = 23
    let result = lsp_request_hover(&mut service, uri.clone(), pos(0, 23)).await;
    let hover = result.expect("Hover should return something");

    // The range should be in UTF-8 (byte offsets)
    // \"emoji\" starts at 23 and ends at 23 + 5 = 28
    let range = hover.range.expect("Hover should have a range");
    assert_eq!(range.start.character, 23);
    assert_eq!(range.end.character, 28);
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_hover_range_with_utf16() {
    let schema = "type Query { field(arg: String): User } type User { emoji: String }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config.base_dir = std::fs::canonicalize(&config.base_dir).unwrap();
    let (mut service, _messages) = create_lsp_service_with_socket(config);

    // 1. Initialize with UTF-16
    let mut params = InitializeParams::default();
    params.capabilities.general = Some(GeneralClientCapabilities {
        position_encodings: Some(vec![PositionEncodingKind::UTF16]),
        ..Default::default()
    });
    let init_req = Request::build("initialize")
        .params(serde_json::to_value(params).unwrap())
        .id(1)
        .finish();
    let _ = Service::call(&mut service, init_req).await.unwrap();

    // Trigger initialized
    let initialized_notif = Request::build("initialized")
        .params(serde_json::json!({}))
        .finish();
    let _ = Service::call(&mut service, initialized_notif)
        .await
        .unwrap();

    wait_for_workspace_scan(service.inner()).await;

    // 2. Open file with emoji
    let text = "{ field(arg: \"🚀\") { emoji } }";
    let uri = write_project_file(&dir, "utf16.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    // 3. Request hover at \"emoji\"
    // { (1) + space (1) + field (5) + ( (1) + arg (3) + : (1) + space (1) + \" (1) + 🚀 (2) + \" (1) + ) (1) + space (1) + { (1) + space (1)
    // = 1 + 1 + 5 + 1 + 3 + 1 + 1 + 1 + 2 + 1 + 1 + 1 + 1 + 1 = 21
    let result = lsp_request_hover(&mut service, uri.clone(), pos(0, 21)).await;
    let hover = result.expect("Hover should return something");

    // The range should be in UTF-16 (code units)
    // \"emoji\" starts at 21 and ends at 21 + 5 = 26
    let range = hover.range.expect("Hover should have a range");
    assert_eq!(range.start.character, 21);
    assert_eq!(range.end.character, 26);
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_hover_range_with_utf32() {
    let schema = "type Query { field(arg: String): User } type User { emoji: String }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config.base_dir = std::fs::canonicalize(&config.base_dir).unwrap();
    let (mut service, _messages) = create_lsp_service_with_socket(config);

    // 1. Initialize with UTF-32
    let mut params = InitializeParams::default();
    params.capabilities.general = Some(GeneralClientCapabilities {
        position_encodings: Some(vec![PositionEncodingKind::UTF32]),
        ..Default::default()
    });
    let init_req = Request::build("initialize")
        .params(serde_json::to_value(params).unwrap())
        .id(1)
        .finish();
    let _ = Service::call(&mut service, init_req).await.unwrap();

    // Trigger initialized
    let initialized_notif = Request::build("initialized")
        .params(serde_json::json!({}))
        .finish();
    let _ = Service::call(&mut service, initialized_notif)
        .await
        .unwrap();

    wait_for_workspace_scan(service.inner()).await;

    // 2. Open file with emoji
    let text = "{ field(arg: \"🚀\") { emoji } }";
    let uri = write_project_file(&dir, "utf32.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    // 3. Request hover at \"emoji\"
    // UTF-32 char offsets:
    // { (1) + space (1) + field (5) + ( (1) + arg (3) + : (1) + space (1) + \" (1) + 🚀 (1) + \" (1) + ) (1) + space (1) + { (1) + space (1)
    // = 1 + 1 + 5 + 1 + 3 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 = 20
    let result = lsp_request_hover(&mut service, uri.clone(), pos(0, 20)).await;
    let hover = result.expect("Hover should return something");

    // The range should be in UTF-32 (char offsets)
    // \"emoji\" starts at 20 and ends at 20 + 5 = 25
    let range = hover.range.expect("Hover should have a range");
    assert_eq!(range.start.character, 20);
    assert_eq!(range.end.character, 25);
}
