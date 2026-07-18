use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use stratum_engine::{Engine, domain::Config};
use tower::ServiceExt;
fn fixture() -> (tempfile::TempDir, axum::Router) {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(Config {
        data_dir: dir.path().join("state"),
        ..Default::default()
    })
    .unwrap();
    (dir, stratum_api::router(engine, "test-token".into()))
}
#[tokio::test]
async fn all_routes_require_auth_and_browser_origins_are_denied() {
    let (_dir, app) = fixture();
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    let r = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/health")
                .header("authorization", "Bearer test-token")
                .header("origin", "https://evil.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
}
#[tokio::test]
async fn health_files_and_structured_errors() {
    let (_dir, app) = fixture();
    for path in [
        "/api/v1/health",
        "/api/v1/files?limit=2",
        "/api/v1/openapi.json",
    ] {
        let r = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let body = r.into_body().collect().await.unwrap().to_bytes();
        let _: serde_json::Value = serde_json::from_slice(&body).unwrap();
    }
    let r = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/cleanup/plans/nope")
                .header("authorization", "Bearer test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
    let body = r.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap()["code"],
        "not_found"
    );
}
#[test]
fn openapi_has_security_typed_actions_and_query_filters() {
    let spec = stratum_api::specification();
    assert!(spec["components"]["securitySchemes"]["localBearer"].is_object());
    assert!(spec["paths"]["/api/v1/storage/breakdown"]["get"].is_object());
    assert!(spec["components"]["schemas"]["InsightMeasurements"].is_object());
    assert!(spec["paths"]["/api/v1/cleanup/plans/{id}/execute"]["post"]["requestBody"].is_object());
    assert!(
        spec["paths"]["/api/v1/files"]["get"]["parameters"]
            .as_array()
            .unwrap()
            .len()
            > 10
    );
}

#[tokio::test]
async fn breakdown_contract_includes_files_and_explicit_remainder() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("data");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("first"), [1; 100]).unwrap();
    std::fs::write(root.join("second"), [2; 50]).unwrap();
    let engine = Engine::open(Config {
        data_dir: dir.path().join("state"),
        ..Default::default()
    })
    .unwrap();
    engine.scan_location(root.to_str().unwrap()).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    let app = stratum_api::router(engine, "test-token".into());
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/storage/breakdown?path={}&limit=1",
                    root.display()
                ))
                .header("authorization", "Bearer test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let data: serde_json::Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(data["children"][0]["kind"], "file");
    assert_eq!(data["omitted_count"], 1);
    assert_eq!(data["omitted_logical_bytes"], 50);
    assert_eq!(data["children_logical_bytes"], 150);
}
#[tokio::test]
async fn malformed_and_unknown_queries_return_stable_errors() {
    let (_dir, app) = fixture();
    for path in ["/api/v1/files?limit=oops", "/api/v1/files?unsupported=true"] {
        let r = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(r.status().is_client_error());
        let body = r.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap()["code"],
            "invalid_request"
        );
    }
}
