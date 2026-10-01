//! HTTP v1 adapters. Every operation delegates to the transport-independent application service.
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Query, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::{convert::Infallible, net::SocketAddr, sync::Arc};
use stratum_domain::*;
use stratum_engine::Engine;
use subtle::ConstantTimeEq;
use utoipa::{OpenApi, ToSchema};

#[derive(Clone)]
pub struct ApiState {
    pub engine: Arc<Engine>,
    token: Arc<String>,
    requests: Arc<tokio::sync::Semaphore>,
}
#[derive(Debug, Serialize, ToSchema)]
pub struct ApiErrorBody {
    pub code: String,
    pub message: String,
}
pub struct ApiError(Error);
impl From<Error> for ApiError {
    fn from(e: Error) -> Self {
        Self(e)
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match self.0.code {
            "not_found" | "path_not_found" => StatusCode::NOT_FOUND,
            "approval_required" | "protected_path" | "permission_denied" => StatusCode::FORBIDDEN,
            "busy" | "conflict" | "filesystem_changed" => StatusCode::CONFLICT,
            "unsupported_platform_feature" => StatusCode::NOT_IMPLEMENTED,
            "database_error" | "internal_error" => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::BAD_REQUEST,
        };
        (
            status,
            Json(ApiErrorBody {
                code: self.0.code.into(),
                message: self.0.message,
            }),
        )
            .into_response()
    }
}
type ApiResult<T> = std::result::Result<Json<T>, ApiError>;
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T> + Send + 'static,
) -> ApiResult<T> {
    Ok(Json(tokio::task::spawn_blocking(f).await.map_err(
        |e| ApiError(Error::new("internal_error", e.to_string())),
    )??))
}
async fn authenticate(State(state): State<ApiState>, request: Request, next: Next) -> Response {
    if request.headers().contains_key("origin") {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiErrorBody {
                code: "browser_origin_forbidden".into(),
                message: "Browser-origin requests are disabled; use a local native client".into(),
            }),
        )
            .into_response();
    }
    let actual = request
        .headers()
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .unwrap_or("");
    if !bool::from(actual.as_bytes().ct_eq(state.token.as_bytes())) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ApiErrorBody {
                code: "unauthorized".into(),
                message: "A local bearer token is required".into(),
            }),
        )
            .into_response();
    }
    let Ok(_permit) = state.requests.try_acquire() else {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(ApiErrorBody {
                code: "busy".into(),
                message: "Too many concurrent API requests".into(),
            }),
        )
            .into_response();
    };
    let response = next.run(request).await;
    if response.status().is_client_error()
        && !response
            .headers()
            .get("content-type")
            .is_some_and(|v| v.to_str().is_ok_and(|s| s.starts_with("application/json")))
    {
        return(response.status(),Json(ApiErrorBody{code:"invalid_request".into(),message:"Invalid path, query, JSON body or route; consult OpenAPI for the request schema".into()})).into_response();
    }
    response
}
pub fn router(engine: Arc<Engine>, token: String) -> Router {
    let state = ApiState {
        engine,
        token: Arc::new(token),
        requests: Arc::new(tokio::sync::Semaphore::new(32)),
    };
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/system", get(system))
        .route("/api/v1/volumes", get(volumes))
        .route("/api/v1/processes", get(processes))
        .route("/api/v1/processes/{pid}", get(process))
        .route("/api/v1/scans", get(scans).post(start_scan))
        .route("/api/v1/scans/{id}/{action}", post(control_scan))
        .route("/api/v1/scans/{id}/warnings", get(warnings))
        .route("/api/v1/jobs/{id}", get(job))
        .route("/api/v1/index/reconcile", post(reconcile))
        .route("/api/v1/files", get(files))
        .route("/api/v1/directories", get(directories))
        .route("/api/v1/storage/largest", get(files))
        .route("/api/v1/storage/categories", get(categories))
        .route("/api/v1/storage/history", get(history))
        .route("/api/v1/storage/explain", get(explain))
        .route("/api/v1/storage/breakdown", get(breakdown))
        .route("/api/v1/files/inspect", get(inspect_entry))
        .route("/api/v1/duplicates", get(duplicates))
        .route("/api/v1/duplicates/groups", get(duplicate_groups))
        .route("/api/v1/duplicates/scan", post(start_duplicates))
        .route("/api/v1/apps", get(apps))
        .route("/api/v1/apps/{id}", get(app))
        .route("/api/v1/apps/{id}/uninstall-plan", post(uninstall))
        .route("/api/v1/insights", get(insights))
        .route("/api/v1/insights/{id}", get(insight))
        .route("/api/v1/cleanup/candidates", get(candidates))
        .route("/api/v1/cleanup/plans", post(plan))
        .route("/api/v1/cleanup/plans/{id}", get(show_plan))
        .route("/api/v1/cleanup/plans/{id}/execute", post(execute))
        .route("/api/v1/cleanup/operations/{id}", get(operation))
        .route("/api/v1/cleanup/operations/{id}/undo", post(undo))
        .route("/api/v1/cleanup/operations/{id}/purge", post(purge))
        .route("/api/v1/audit", get(audit))
        .route("/api/v1/system/history", get(system_history))
        .route("/api/v1/events", get(events))
        .route("/api/v1/openapi.json", get(openapi))
        .layer(DefaultBodyLimit::max(64 * 1024))
        .layer(middleware::from_fn_with_state(state.clone(), authenticate))
        .with_state(state)
}
#[utoipa::path(get,path="/api/v1/health",responses((status=200,body=serde_json::Value)))]
async fn health(State(s): State<ApiState>) -> ApiResult<serde_json::Value> {
    blocking(move||Ok(serde_json::json!({"status":"ok","version":env!("CARGO_PKG_VERSION"),"api_version":"v1","privacy":"local_only","watcher_running":s.engine.watcher_running(),"indexed_roots":s.engine.roots()?}))).await
}
#[utoipa::path(post,path="/api/v1/index/reconcile",request_body=ReconcileRequest,responses((status=200,body=ReconcileReport)))]
async fn reconcile(
    State(s): State<ApiState>,
    Json(r): Json<ReconcileRequest>,
) -> ApiResult<ReconcileReport> {
    blocking(move || s.engine.reconcile_paths(r)).await
}
#[utoipa::path(get,path="/api/v1/system",responses((status=200,body=SystemSnapshot)))]
async fn system(State(s): State<ApiState>) -> ApiResult<SystemSnapshot> {
    blocking(move || Ok(s.engine.system())).await
}
#[utoipa::path(get,path="/api/v1/volumes",responses((status=200,body=Vec<Volume>)))]
async fn volumes(State(s): State<ApiState>) -> ApiResult<Vec<Volume>> {
    blocking(move || Ok(s.engine.system().volumes)).await
}
#[utoipa::path(get,path="/api/v1/processes",responses((status=200,body=Vec<ProcessSnapshot>)))]
async fn processes(State(s): State<ApiState>) -> ApiResult<Vec<ProcessSnapshot>> {
    blocking(move || Ok(s.engine.system().processes)).await
}
#[utoipa::path(get,path="/api/v1/processes/{pid}",params(("pid"=u32,Path)),responses((status=200,body=ProcessSnapshot)))]
async fn process(State(s): State<ApiState>, Path(pid): Path<u32>) -> ApiResult<ProcessSnapshot> {
    blocking(move || {
        s.engine
            .system()
            .processes
            .into_iter()
            .find(|p| p.pid == pid)
            .ok_or_else(|| Error::new("not_found", "Process not visible"))
    })
    .await
}
#[utoipa::path(get,path="/api/v1/scans",responses((status=200,body=Vec<ScanRecord>)))]
async fn scans(State(s): State<ApiState>) -> ApiResult<Vec<ScanRecord>> {
    blocking(move || s.engine.scans()).await
}
#[utoipa::path(post,path="/api/v1/scans",request_body=ScanRequest,responses((status=200,body=Job)))]
async fn start_scan(State(s): State<ApiState>, Json(r): Json<ScanRequest>) -> ApiResult<Job> {
    blocking(move || s.engine.start_scan_job(r)).await
}
#[utoipa::path(get,path="/api/v1/jobs/{id}",params(("id"=String,Path)),responses((status=200,body=Job)))]
async fn job(State(s): State<ApiState>, Path(id): Path<String>) -> ApiResult<Job> {
    blocking(move || s.engine.job(&id)).await
}
#[utoipa::path(post,path="/api/v1/scans/{id}/{action}",params(("id"=String,Path),("action"=String,Path)),responses((status=200,body=serde_json::Value)))]
async fn control_scan(
    State(s): State<ApiState>,
    Path((id, action)): Path<(String, String)>,
) -> ApiResult<serde_json::Value> {
    s.engine.control_scan(&id, &action)?;
    Ok(Json(serde_json::json!({"scan_id":id,"action":action})))
}
#[utoipa::path(get,path="/api/v1/scans/{id}/warnings",params(("id"=String,Path)),responses((status=200,body=Vec<Evidence>)))]
async fn warnings(State(s): State<ApiState>, Path(id): Path<String>) -> ApiResult<Vec<Evidence>> {
    blocking(move || s.engine.warnings(&id)).await
}
#[utoipa::path(get,path="/api/v1/files",responses((status=200,body=Page<Entry>)))]
async fn files(State(s): State<ApiState>, Query(q): Query<FileQuery>) -> ApiResult<Page<Entry>> {
    blocking(move || s.engine.files(&q)).await
}
#[utoipa::path(get,path="/api/v1/directories",responses((status=200,body=Page<Entry>)))]
async fn directories(
    State(s): State<ApiState>,
    Query(mut q): Query<FileQuery>,
) -> ApiResult<Page<Entry>> {
    q.kind = Some("directory".into());
    blocking(move || s.engine.files(&q)).await
}
#[utoipa::path(get,path="/api/v1/storage/categories",responses((status=200,body=Vec<CategoryTotal>)))]
async fn categories(State(s): State<ApiState>) -> ApiResult<Vec<CategoryTotal>> {
    blocking(move || s.engine.categories()).await
}
#[derive(Deserialize)]
pub struct HistoryQuery {
    path: Option<String>,
    since: Option<i64>,
}
#[utoipa::path(get,path="/api/v1/storage/history",params(("path"=Option<String>,Query),("since"=Option<i64>,Query)),responses((status=200,body=Vec<HistoryPoint>)))]
async fn history(
    State(s): State<ApiState>,
    Query(q): Query<HistoryQuery>,
) -> ApiResult<Vec<HistoryPoint>> {
    blocking(move || s.engine.history(q.path.as_deref(), q.since.unwrap_or(0))).await
}
#[utoipa::path(get,path="/api/v1/storage/explain",responses((status=200,body=StorageExplanation)))]
async fn explain(State(s): State<ApiState>) -> ApiResult<StorageExplanation> {
    blocking(move || s.engine.explain_storage()).await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PathQuery {
    path: String,
    limit: Option<u32>,
}
#[utoipa::path(get,path="/api/v1/storage/breakdown",params(("path"=String,Query),("limit"=Option<u32>,Query)),responses((status=200,body=DirectoryBreakdown)))]
async fn breakdown(
    State(s): State<ApiState>,
    Query(q): Query<PathQuery>,
) -> ApiResult<DirectoryBreakdown> {
    blocking(move || s.engine.directory_breakdown(&q.path, q.limit.unwrap_or(60))).await
}
#[utoipa::path(get,path="/api/v1/files/inspect",params(("path"=String,Query)),responses((status=200,body=Entry)))]
async fn inspect_entry(State(s): State<ApiState>, Query(q): Query<PathQuery>) -> ApiResult<Entry> {
    blocking(move || s.engine.inspect_entry(&q.path)).await
}
#[utoipa::path(get,path="/api/v1/duplicates",responses((status=200,body=DuplicateReport)))]
async fn duplicates(State(s): State<ApiState>) -> ApiResult<DuplicateReport> {
    blocking(move || s.engine.duplicates()).await
}
#[utoipa::path(get,path="/api/v1/duplicates/groups",params(("limit"=Option<u32>,Query),("offset"=Option<u64>,Query)),responses((status=200,body=Page<DuplicateGroup>)))]
async fn duplicate_groups(
    State(s): State<ApiState>,
    Query(q): Query<FileQuery>,
) -> ApiResult<Page<DuplicateGroup>> {
    blocking(move || s.engine.duplicate_groups(q.limit, q.offset)).await
}
#[utoipa::path(post,path="/api/v1/duplicates/scan",responses((status=200,body=Job)))]
async fn start_duplicates(State(s): State<ApiState>) -> ApiResult<Job> {
    blocking(move || s.engine.start_duplicate_job()).await
}
#[utoipa::path(get,path="/api/v1/apps",responses((status=200,body=Vec<Application>)))]
async fn apps(State(s): State<ApiState>) -> ApiResult<Vec<Application>> {
    blocking(move || s.engine.applications()).await
}
#[utoipa::path(get,path="/api/v1/apps/{id}",params(("id"=String,Path)),responses((status=200,body=Application)))]
async fn app(State(s): State<ApiState>, Path(id): Path<String>) -> ApiResult<Application> {
    blocking(move || s.engine.inspect_application(&id)).await
}
#[utoipa::path(post,path="/api/v1/apps/{id}/uninstall-plan",params(("id"=String,Path)),responses((status=200,body=serde_json::Value)))]
async fn uninstall(
    State(s): State<ApiState>,
    Path(id): Path<String>,
) -> ApiResult<serde_json::Value> {
    blocking(move || s.engine.uninstall_plan(&id)).await
}
#[utoipa::path(get,path="/api/v1/insights",responses((status=200,body=Vec<Insight>)))]
async fn insights(State(s): State<ApiState>) -> ApiResult<Vec<Insight>> {
    blocking(move || s.engine.insights()).await
}
#[utoipa::path(get,path="/api/v1/insights/{id}",params(("id"=String,Path)),responses((status=200,body=Insight)))]
async fn insight(State(s): State<ApiState>, Path(id): Path<String>) -> ApiResult<Insight> {
    blocking(move || {
        s.engine
            .insights()?
            .into_iter()
            .find(|i| i.id == id)
            .ok_or_else(|| Error::new("not_found", "Insight not found"))
    })
    .await
}
#[utoipa::path(get,path="/api/v1/cleanup/candidates",responses((status=200,body=Page<CleanupCandidate>)))]
async fn candidates(
    State(s): State<ApiState>,
    Query(q): Query<FileQuery>,
) -> ApiResult<Page<CleanupCandidate>> {
    blocking(move || s.engine.cleanup_candidates(&q)).await
}
#[utoipa::path(post,path="/api/v1/cleanup/plans",request_body=PlanRequest,responses((status=200,body=CleanupPlan)))]
async fn plan(State(s): State<ApiState>, Json(r): Json<PlanRequest>) -> ApiResult<CleanupPlan> {
    blocking(move || s.engine.create_cleanup_plan(r)).await
}
#[utoipa::path(get,path="/api/v1/cleanup/plans/{id}",params(("id"=String,Path)),responses((status=200,body=CleanupPlan)))]
async fn show_plan(State(s): State<ApiState>, Path(id): Path<String>) -> ApiResult<CleanupPlan> {
    blocking(move || s.engine.cleanup_plan(&id)).await
}
#[utoipa::path(post,path="/api/v1/cleanup/plans/{id}/execute",params(("id"=String,Path)),request_body=ExecuteRequest,responses((status=200,body=CleanupOperation)))]
async fn execute(
    State(s): State<ApiState>,
    Path(id): Path<String>,
    Json(r): Json<ExecuteRequest>,
) -> ApiResult<CleanupOperation> {
    blocking(move || s.engine.execute_cleanup_plan(&id, &r.approval)).await
}
#[utoipa::path(get,path="/api/v1/cleanup/operations/{id}",params(("id"=String,Path)),responses((status=200,body=CleanupOperation)))]
async fn operation(
    State(s): State<ApiState>,
    Path(id): Path<String>,
) -> ApiResult<CleanupOperation> {
    blocking(move || s.engine.cleanup_operation(&id)).await
}
#[utoipa::path(post,path="/api/v1/cleanup/operations/{id}/undo",params(("id"=String,Path)),responses((status=200,body=CleanupOperation)))]
async fn undo(State(s): State<ApiState>, Path(id): Path<String>) -> ApiResult<CleanupOperation> {
    blocking(move || s.engine.undo_cleanup(&id)).await
}
/// Permanently delete what an operation still holds in quarantine.
///
/// The approval must be `PURGE <operation-id>`, separate from the quarantine phrase. Files that
/// changed since they moved are left in place. A purge cannot be undone.
#[utoipa::path(post,path="/api/v1/cleanup/operations/{id}/purge",params(("id"=String,Path)),request_body=ExecuteRequest,responses((status=200,body=CleanupOperation)))]
async fn purge(
    State(s): State<ApiState>,
    Path(id): Path<String>,
    Json(r): Json<ExecuteRequest>,
) -> ApiResult<CleanupOperation> {
    blocking(move || s.engine.purge_quarantine(&id, &r.approval)).await
}
#[utoipa::path(get,path="/api/v1/audit",params(("limit"=Option<u32>,Query),("offset"=Option<u64>,Query)),responses((status=200,body=Vec<AuditRecord>)))]
async fn audit(
    State(s): State<ApiState>,
    Query(q): Query<FileQuery>,
) -> ApiResult<Vec<AuditRecord>> {
    blocking(move || s.engine.audit(q.limit, q.offset)).await
}
#[utoipa::path(get,path="/api/v1/system/history",responses((status=200,body=Vec<serde_json::Value>)))]
async fn system_history(State(s): State<ApiState>) -> ApiResult<Vec<serde_json::Value>> {
    blocking(move || s.engine.system_history()).await
}
#[utoipa::path(get,path="/api/v1/events",responses((status=200,description="SSE structured OperationEvent stream; lag emits resync_required")))]
async fn events(
    State(s): State<ApiState>,
) -> Sse<impl futures_util::Stream<Item = std::result::Result<Event, Infallible>>> {
    let stream = tokio_stream::wrappers::BroadcastStream::new(s.engine.subscribe()).map(|event| {
        Ok(match event {
            Ok(e) => Event::default()
                .json_data(e)
                .unwrap_or_else(|_| Event::default().event("serialization_error")),
            Err(_) => Event::default()
                .event("resync_required")
                .data("Event consumer lagged; query current operation status"),
        })
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}
#[derive(OpenApi)]
#[openapi(
    paths(
        reconcile,
        health,
        system,
        volumes,
        processes,
        process,
        scans,
        start_scan,
        job,
        control_scan,
        warnings,
        files,
        directories,
        categories,
        history,
        explain,
        breakdown,
        inspect_entry,
        duplicates,
        duplicate_groups,
        start_duplicates,
        apps,
        app,
        uninstall,
        insights,
        insight,
        candidates,
        plan,
        show_plan,
        execute,
        operation,
        undo,
        purge,
        audit,
        system_history,
        events
    ),
    components(schemas(ApiErrorBody, FileQuery, OperationEvent)),
    info(title = "Stratum local machine intelligence API", version = "1.0.0")
)]
pub struct ApiDoc;
pub fn specification() -> serde_json::Value {
    let mut doc = serde_json::to_value(ApiDoc::openapi()).expect("OpenAPI is serializable");
    doc["components"]["securitySchemes"] = serde_json::json!({"localBearer":{"type":"http","scheme":"bearer","description":"Local token file, mode 0600"}});
    doc["security"] = serde_json::json!([{"localBearer":[]}]);
    let query_schema = doc["components"]["schemas"]["FileQuery"].clone();
    for path in [
        "/api/v1/files",
        "/api/v1/directories",
        "/api/v1/cleanup/candidates",
    ] {
        if let Some(properties) = query_schema["properties"].as_object() {
            doc["paths"][path]["get"]["parameters"]=serde_json::Value::Array(properties.iter().map(|(name,schema)|serde_json::json!({"name":name,"in":"query","required":false,"schema":schema})).collect());
        }
    }
    let largest = doc["paths"]["/api/v1/files"].clone();
    doc["paths"]["/api/v1/storage/largest"] = largest;
    doc["paths"]["/api/v1/storage/largest"]["get"]["operationId"] = "largest".into();
    doc["paths"]["/api/v1/openapi.json"] = serde_json::json!({"get":{"operationId":"openapi","responses":{"200":{"description":"Generated OpenAPI document","content":{"application/json":{"schema":{"type":"object"}}}}}}});
    if let Some(paths) = doc["paths"].as_object_mut() {
        for path in paths.values_mut() {
            if let Some(operations) = path.as_object_mut() {
                for operation in operations.values_mut() {
                    operation["responses"]["default"] = serde_json::json!({"description":"Stable machine-readable domain or request error","content":{"application/json":{"schema":{"$ref":"#/components/schemas/ApiErrorBody"}}}});
                }
            }
        }
    }
    doc
}
async fn openapi() -> Json<serde_json::Value> {
    Json(specification())
}

pub fn local_token(engine: &Engine) -> Result<String> {
    use std::io::Write;
    let path = engine.config.data_dir.join("api.token");
    if path.exists() {
        let token = std::fs::read_to_string(path)?;
        if token.len() < 32
            || !token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(Error::invalid("Token must contain at least 32 characters"));
        }
        return Ok(token);
    }
    let token = format!("{}{}", id(), id());
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(token.as_bytes())?;
    file.sync_all()?;
    Ok(token)
}

/// Small bounded health probe for local service discovery; never follows redirects.
pub fn probe_health(engine: &Engine) -> Result<serde_json::Value> {
    use std::io::{Read, Write};
    let address: SocketAddr = engine
        .config
        .api_bind
        .parse()
        .map_err(|_| Error::invalid("Invalid API address"))?;
    if !address.ip().is_loopback() {
        return Err(Error::new(
            "protected_path",
            "Only loopback services can be queried",
        ));
    }
    let token = local_token(engine)?;
    let mut stream =
        std::net::TcpStream::connect_timeout(&address, std::time::Duration::from_secs(2))
            .map_err(|e| Error::new("daemon_unavailable", e.to_string()))?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(std::time::Duration::from_secs(2)))?;
    write!(
        stream,
        "GET /api/v1/health HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n"
    )?;
    let mut response = String::new();
    stream.take(65536).read_to_string(&mut response)?;
    let (head, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| Error::new("api_error", "Invalid health response"))?;
    if !head.starts_with("HTTP/1.1 200") {
        return Err(Error::new("api_error", "Service health request failed"));
    }
    Ok(serde_json::from_str(body)?)
}
pub async fn serve(
    engine: Arc<Engine>,
    address: SocketAddr,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> Result<()> {
    if !address.ip().is_loopback() {
        return Err(Error::new(
            "protected_path",
            "Only loopback API binding is supported",
        ));
    }
    let token = local_token(&engine)?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    tracing::info!(address=%listener.local_addr()?,"Stratum local API listening");
    axum::serve(listener, router(engine, token))
        .with_graceful_shutdown(shutdown)
        .await
        .map_err(Error::io)
}
