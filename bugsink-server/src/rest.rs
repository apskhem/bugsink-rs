//! Upstream's REST API, `/api/canonical/0/` (Django REST Framework in upstream Bugsink 2.6.3), for scripts and
//! integrations: issues (and triaging them), events, comments and history, projects, teams and releases.
//!
//! Requests carry an auth token (`Authorization: Bearer <40 hex>`, from the auth tokens page) whose capabilities and
//! bindings (to a user, to a project) decide what it may do, as upstream's `token_guard`s do. Responses, errors and
//! pagination cursors are upstream's, down to the messages; `tools/parity/rest_api.sh` compares the two.

mod events;
mod fields;
mod issues;
pub(crate) mod markdown;
mod pagination;
mod projects;
mod pyjson;

use std::collections::BTreeSet;
use std::future::Future;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{
  FromRequest,
  State,
};
use axum::http::{
  HeaderMap,
  HeaderValue,
  Method,
  StatusCode,
  header,
};
use axum::response::{
  IntoResponse,
  Response,
};
use axum::routing::any;
use serde_json::{
  Value,
  json,
};
use uuid::Uuid;

use crate::accounts::User;
use crate::api::ApiState;
use crate::forms::REQUIRED;
use crate::installation::CAPABILITIES;
use crate::py::{
  PyInt,
  drf_datetime,
  py_int,
  py_uuid,
  strip,
};

pub const PREFIX: &str = "/api/canonical/0";

pub fn routes() -> Router<ApiState> {
  let r = |path: &str| format!("{PREFIX}{path}");
  Router::new()
    .route(&r("/"), any(root))
    .route(&r("/events/"), any(events::list))
    .route(&r("/events/lookup/"), any(events::lookup))
    .route(&r("/events/{pk}/"), any(events::retrieve))
    .route(&r("/events/{pk}/stacktrace/"), any(events::stacktrace))
    .route(&r("/issue-comments/"), any(issues::comment))
    .route(&r("/issue-history/"), any(issues::history))
    .route(&r("/issues/"), any(issues::list))
    .route(&r("/issues/{pk}/"), any(issues::retrieve))
    .route(&r("/issues/{pk}/tags/"), any(issues::tags))
    .route(&r("/issues/{pk}/grouping/"), any(issues::grouping))
    .route(&r("/issues/{pk}/event-counts/"), any(issues::event_counts))
    .route(&r("/issues/{pk}/{action}/"), any(issues::action))
    .route(&r("/projects/"), any(projects::projects))
    .route(&r("/projects/{pk}/"), any(projects::project))
    .route(&r("/projects/{pk}/event-counts/"), any(projects::project_event_counts))
    .route(&r("/releases/"), any(projects::releases))
    .route(&r("/releases/{pk}/"), any(projects::release))
    .route(&r("/teams/"), any(projects::teams))
    .route(&r("/teams/{pk}/"), any(projects::team))
    .route(&r("/schema/"), axum::routing::get(schema))
    .route(&r("/schema/swagger-ui/"), axum::routing::get(swagger_ui))
    .route(
      &r("/{*rest}"),
      any(|| async { Drf::detail(StatusCode::NOT_FOUND, "Not found.").into_response() }),
    )
}

// --- requests -----------------------------------------------------------------------------------------------------

/// What upstream's views look at in a request.
pub struct Request {
  pub method: Method,
  pub headers: HeaderMap,
  /// The path as received (percent-encoded).
  raw_path: String,
  raw_query: String,
  pub query: Query,
  body: Bytes,
  /// `scheme://host`, as Django's `build_absolute_uri` sees it.
  origin: String,
}

impl FromRequest<ApiState> for Request {
  type Rejection = Response;

  async fn from_request(req: axum::extract::Request, state: &ApiState) -> Result<Self, Self::Rejection> {
    let (parts, body) = req.into_parts();
    let body = axum::body::to_bytes(body, 50 * 1024 * 1024)
      .await
      .map_err(|e| Drf::detail(StatusCode::BAD_REQUEST, &format!("Unreadable request body: {e}")).into_response())?;
    let header = |name: &str| parts.headers.get(name).map(|v| latin1(v.as_bytes()));
    let proxy = &state.settings.proxy;
    let behind_proxy = proxy.behind_https_proxy || proxy.behind_plain_http_proxy;
    // `SECURE_PROXY_SSL_HEADER` and `USE_X_FORWARDED_HOST`, which upstream sets only behind a proxy
    let forwarded_proto = header("x-forwarded-proto").filter(|_| behind_proxy);
    let https = forwarded_proto.is_some_and(|proto| strip(proto.split(',').next().unwrap_or("")) == "https");
    let scheme = if https { "https" } else { "http" };
    let forwarded_host = header("x-forwarded-host").filter(|_| behind_proxy && proxy.use_x_forwarded_host);
    let host = forwarded_host.or_else(|| header("host")).unwrap_or_default();
    let raw_query = parts.uri.query().unwrap_or("").to_owned();
    Ok(Self {
      method: parts.method,
      raw_path: parts.uri.path().to_owned(),
      query: Query(url_parse_qsl(&raw_query)),
      raw_query,
      headers: parts.headers,
      body,
      origin: format!("{scheme}://{host}"),
    })
  }
}

impl Request {
  /// The method a view sees: GET and HEAD alike.
  fn view_method(&self) -> &str {
    if self.method == Method::HEAD {
      "GET"
    } else {
      self.method.as_str()
    }
  }

  /// `scheme://host/path` (Django's `escape_uri_path` of the decoded path).
  pub fn absolute_path(&self) -> String {
    let decoded = String::from_utf8_lossy(&percent_decode(self.raw_path.as_bytes())).into_owned();
    format!("{}{}", self.origin, quote(&decoded, b"/:@&+$,-_.!~*'()"))
  }

  /// An absolute URL for a path under the API.
  pub fn url(&self, path: &str) -> String {
    format!("{}{PREFIX}{path}", self.origin)
  }

  /// `request.data` for a JSON body: `{}` when there is none.
  pub fn data(&self) -> Result<Value, Drf> {
    if self.body.is_empty() {
      return Ok(json!({}));
    }
    let content_type = self
      .headers
      .get(header::CONTENT_TYPE)
      .map(|v| latin1(v.as_bytes()))
      .unwrap_or_default();
    // DRF's media type match: no case folding, wildcards allowed
    let full_type = strip(content_type.split(';').next().unwrap_or(""));
    let (main, sub) = full_type.split_once('/').unwrap_or((full_type, ""));
    let (main, sub) = (strip(main), strip(sub));
    if !((main == "application" || main == "*") && (sub == "json" || sub == "*")) {
      return Err(Drf::detail(
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        &format!("Unsupported media type \"{content_type}\" in request."),
      ));
    }
    pyjson::loads(&self.body).map_err(|e| Drf::detail(StatusCode::BAD_REQUEST, &format!("JSON parse error - {e}")))
  }
}

/// Header values as WSGI hands them to Django: one character per byte.
fn latin1(bytes: &[u8]) -> String {
  bytes.iter().map(|b| char::from(*b)).collect()
}

/// `request.query_params` (Django's `QueryDict`).
pub struct Query(Vec<(String, String)>);

impl Query {
  /// The last value given for `key`.
  pub fn get(&self, key: &str) -> Option<&str> {
    self.0.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
  }

  pub fn get_list(&self, key: &str) -> Vec<&str> {
    self
      .0
      .iter()
      .filter(|(k, _)| k == key)
      .map(|(_, v)| v.as_str())
      .collect()
  }

  pub fn contains(&self, key: &str) -> bool {
    self.0.iter().any(|(k, _)| k == key)
  }
}

/// Python's `parse_qsl(qs, keep_blank_values=True)`.
pub fn url_parse_qsl(qs: &str) -> Vec<(String, String)> {
  let unquote = |s: &str| String::from_utf8_lossy(&percent_decode(s.replace('+', " ").as_bytes())).into_owned();
  qs.split('&')
    .filter(|pair| !pair.is_empty())
    .map(|pair| {
      let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
      (unquote(k), unquote(v))
    })
    .collect()
}

/// `%XX` sequences decoded; malformed ones kept as they are.
fn percent_decode(s: &[u8]) -> Vec<u8> {
  let mut out = Vec::with_capacity(s.len());
  let mut i = 0;
  while i < s.len() {
    let hex = |b: u8| (b as char).to_digit(16);
    if s[i] == b'%'
      && let (Some(h), Some(l)) = (s.get(i + 1).and_then(|b| hex(*b)), s.get(i + 2).and_then(|b| hex(*b)))
    {
      out.push((h * 16 + l) as u8);
      i += 3;
    } else {
      out.push(s[i]);
      i += 1;
    }
  }
  out
}

/// Python's `quote(s, safe)` (UTF-8, upper-case escapes).
fn quote(s: &str, safe: &[u8]) -> String {
  let mut out = String::new();
  for b in s.bytes() {
    if b.is_ascii_alphanumeric() || b"_.-~".contains(&b) || safe.contains(&b) {
      out.push(b as char);
    } else {
      out.push_str(&format!("%{b:02X}"));
    }
  }
  out
}

/// Python's `quote_plus(s)`.
pub fn quote_plus(s: &str) -> String {
  quote(s, b" ").replace(' ', "+")
}

// --- responses and errors -----------------------------------------------------------------------------------------

/// A response body as DRF's `JSONRenderer` writes it.
pub fn render(status: StatusCode, body: &Value) -> Response {
  (
    status,
    [(header::CONTENT_TYPE, "application/json")],
    pyjson::dumps(body),
  )
    .into_response()
}

/// DRF's exceptions: a status and a body (`{"detail": ...}`, or a serializer's errors).
#[derive(Debug)]
pub struct Drf {
  status: StatusCode,
  body: Value,
}

impl Drf {
  pub fn detail(status: StatusCode, detail: &str) -> Self {
    Self {
      status,
      body: json!({ "detail": detail }),
    }
  }

  /// `NotFound()`: what a malformed identifier gets.
  pub fn not_found() -> Self {
    Self::detail(StatusCode::NOT_FOUND, "Not found.")
  }

  /// `get_object_or_404`'s message.
  pub fn no_match(model: &str) -> Self {
    Self::detail(StatusCode::NOT_FOUND, &format!("No {model} matches the given query."))
  }

  pub fn forbidden(detail: &str) -> Self {
    Self::detail(StatusCode::FORBIDDEN, detail)
  }

  /// A `ValidationError`: 400 with the body as is.
  pub fn invalid(body: Value) -> Self {
    Self {
      status: StatusCode::BAD_REQUEST,
      body,
    }
  }

  pub fn internal(e: impl std::fmt::Display) -> Self {
    tracing::error!("REST API: {e}");
    Self::detail(StatusCode::INTERNAL_SERVER_ERROR, "A server error occurred.")
  }
}

impl From<sqlx::Error> for Drf {
  fn from(e: sqlx::Error) -> Self {
    Self::internal(e)
  }
}

impl IntoResponse for Drf {
  fn into_response(self) -> Response {
    let mut response = render(self.status, &self.body);
    if self.status == StatusCode::UNAUTHORIZED {
      response
        .headers_mut()
        .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
    }
    response
  }
}

pub type Reply = Result<Response, Drf>;

pub fn ok(body: Value) -> Reply {
  Ok(render(StatusCode::OK, &body))
}

// --- auth tokens --------------------------------------------------------------------------------------------------

/// An auth token that authenticated a request.
pub struct Token {
  /// A personal token acts as its user, within what the user may do.
  pub user: Option<User>,
  /// A project-bound token only reaches its project.
  pub project_id: Option<i64>,
  capabilities: BTreeSet<&'static str>,
}

impl Token {
  pub fn has(&self, capability: &str) -> bool {
    self.capabilities.contains(capability)
  }
}

/// `BearerTokenAuthentication`: None without a bearer header; Err for a bad one.
async fn authenticate(state: &ApiState, headers: &HeaderMap) -> Result<Option<Token>, Drf> {
  let Some(header) = headers.get(header::AUTHORIZATION).map(|v| latin1(v.as_bytes())) else {
    return Ok(None);
  };
  let Some(raw) = header.strip_prefix("Bearer ") else {
    return Ok(None);
  };
  let failed = |detail: &str| Drf::detail(StatusCode::UNAUTHORIZED, detail);
  let raw = strip(raw);
  if let Some((hint, _)) = raw.split_once(' ')
    && hint.chars().count() <= 20
  {
    return Err(failed(&format!("Invalid Authorization: 'Bearer {hint} ...'")));
  }
  if raw.chars().count() != 40 || !raw.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')) {
    return Err(failed("Malformed Bearer token, must be 40 lowercase hex chars."));
  }
  let columns: Vec<String> = CAPABILITIES
    .iter()
    .map(|(c, _)| format!("t.{}", c.replace([':', '-'], "_")))
    .collect();
  let row = sqlx::query(sqlx::AssertSqlSafe(format!(
    "SELECT t.is_user_bound, t.user_id, u.is_active, t.is_project_bound, t.project_id, p.is_deleted, {}
         FROM bsmain_authtoken t LEFT JOIN auth_user u ON u.id = t.user_id
         LEFT JOIN projects_project p ON p.id = t.project_id
         WHERE t.token = $1 AND (t.expires_at IS NULL OR t.expires_at > now())",
    columns.join(", ")
  )))
  .bind(raw)
  .fetch_optional(&state.pool)
  .await?;
  let invalid = || failed("Invalid Bearer token.");
  let Some(row) = row else { return Err(invalid()) };
  use sqlx::Row;
  let capabilities: BTreeSet<&'static str> = CAPABILITIES
    .iter()
    .filter(|(c, _)| row.get::<bool, _>(c.replace([':', '-'], "_").as_str()))
    .map(|(c, _)| *c)
    .collect();
  let user_id: Option<i64> = row.get("user_id");
  let project_id: Option<i64> = row.get("project_id");
  // `AuthToken.clean`: a consistent token, its user active, its project not deleted
  let user_ok = if row.get("is_user_bound") {
    user_id.is_some() && row.get::<Option<bool>, _>("is_active") == Some(true)
  } else {
    user_id.is_none()
  };
  let project_ok = if row.get("is_project_bound") {
    project_id.is_some()
      && row.get::<Option<bool>, _>("is_deleted") == Some(false)
      && !["projects:manage", "teams:read", "teams:manage"]
        .iter()
        .any(|c| capabilities.contains(c))
  } else {
    project_id.is_none()
  };
  if !user_ok || !project_ok {
    return Err(invalid());
  }
  let user = match user_id {
    None => None,
    Some(id) => {
      Some(
        sqlx::query_as(
          "SELECT id, username, email, first_name, last_name, is_superuser, is_staff, send_email_alerts,
                        theme_preference, language
                 FROM auth_user WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&state.pool)
        .await?,
      )
    }
  };
  Ok(Some(Token {
    user,
    project_id,
    capabilities,
  }))
}

// --- dispatch -----------------------------------------------------------------------------------------------------

/// One of upstream's routes: the methods it maps, with the capability each needs (None: just a token), and the
/// `Allow` header DRF sends.
pub struct Route {
  pub allow: &'static str,
  pub name: &'static str,
  pub methods: &'static [(&'static str, Option<&'static str>)],
}

/// A request that passed authentication and the capability check.
pub struct Ctx {
  pub state: ApiState,
  pub req: Request,
  pub token: Token,
}

impl Ctx {
  pub fn pool(&self) -> &bugsink_db::PgPool {
    &self.state.pool
  }

  /// GET and HEAD alike.
  pub fn method(&self) -> &str {
    self.req.view_method()
  }
}

/// DRF's `initial()` before the view: authentication, then the token's capability for the method; methods the
/// route doesn't map are refused only after that. Every response carries the route's `Allow`.
pub async fn serve<F, Fut>(state: ApiState, req: Request, route: &'static Route, view: F) -> Response
where
  F: FnOnce(Ctx) -> Fut,
  Fut: Future<Output = Reply>,
{
  let mut response = match enter(&state, &req, route).await {
    Ok(token) if req.method == Method::OPTIONS => options(route, &token),
    Ok(token) => {
      match view(Ctx { state, req, token }).await {
        Ok(response) => response,
        Err(e) => e.into_response(),
      }
    }
    Err(e) => e.into_response(),
  };
  response
    .headers_mut()
    .insert(header::ALLOW, HeaderValue::from_static(route.allow));
  response
}

/// The token, if it may go on (OPTIONS needs no capability).
async fn enter(state: &ApiState, req: &Request, route: &Route) -> Result<Token, Drf> {
  let Some(token) = authenticate(state, &req.headers).await? else {
    return Err(Drf::detail(
      StatusCode::UNAUTHORIZED,
      "Authentication credentials were not provided.",
    ));
  };
  if req.method == Method::OPTIONS {
    return Ok(token);
  }
  match route.methods.iter().find(|(m, _)| *m == req.view_method()) {
    None => {
      Err(Drf::detail(
        StatusCode::METHOD_NOT_ALLOWED,
        &format!("Method \"{}\" not allowed.", req.method.as_str()),
      ))
    }
    Some((_, Some(capability))) if !token.has(capability) => {
      Err(Drf::forbidden(&format!(
        "This token does not have the required capability: {capability}."
      )))
    }
    Some(_) => Ok(token),
  }
}

/// DRF's `SimpleMetadata`: the route's name and formats and, if the token may POST there, a description of the
/// fields of the viewset's default serializer (upstream's, as it describes them: `options-actions.json`).
fn options(route: &Route, token: &Token) -> Response {
  let mut metadata = json!({
      "name": route.name,
      "description": "",
      "renders": ["application/json"],
      "parses": ["application/json"],
  });
  let key = match route.name {
    "Team List" => "teams",
    "Project List" => "projects",
    "Issue Comment List" => "issue-comments",
    "Release List" => "releases",
    _ => "issue-action",
  };
  let may_post = route
    .methods
    .iter()
    .any(|(m, c)| *m == "POST" && c.is_none_or(|c| token.has(c)));
  if may_post {
    static ACTIONS: std::sync::LazyLock<Value> =
      std::sync::LazyLock::new(|| serde_json::from_str(include_str!("rest/options-actions.json")).expect("valid JSON"));
    metadata["actions"] = ACTIONS[key].clone();
  }
  render(StatusCode::OK, &metadata)
}

static ROOT: Route = Route {
  allow: "GET, HEAD, OPTIONS",
  name: "Canonical API Root",
  methods: &[("GET", None)],
};

/// `CanonicalAPIRootView`: links to the lists.
async fn root(State(state): State<ApiState>, req: Request) -> Response {
  serve(state, req, &ROOT, |c| {
    async move {
      let mut links = serde_json::Map::new();
      for name in [
        "events",
        "issue-comments",
        "issue-history",
        "issues",
        "projects",
        "releases",
        "teams",
      ] {
        links.insert(name.into(), json!(c.req.url(&format!("/{name}/"))));
      }
      ok(Value::Object(links))
    }
  })
  .await
}

/// The OpenAPI description of this API (upstream's, which drf-spectacular generates from its code): YAML, or JSON with
/// `?format=json` or `Accept: application/json`. No token needed.
async fn schema(req: Request) -> Response {
  let accept = req
    .headers
    .get(header::ACCEPT)
    .map(|v| latin1(v.as_bytes()))
    .unwrap_or_default();
  // errors are rendered as YAML too
  let yaml_error = |status: StatusCode, detail: &str, code: &str| {
    let body = format!("detail:\n  string: {detail}\n  code: {code}\n");
    let headers = [
      (header::CONTENT_TYPE, "application/vnd.oai.openapi; charset=utf-8"),
      (header::ALLOW, "GET, HEAD, OPTIONS"),
    ];
    (status, headers, body).into_response()
  };
  let (json, content_type) = match req.query.get("format") {
    Some("json") => (true, "application/vnd.oai.openapi+json"),
    Some("yaml") => (false, "application/vnd.oai.openapi; charset=utf-8"),
    Some(_) => return yaml_error(StatusCode::NOT_FOUND, "Not found.", "not_found"),
    None if accept.contains("application/json") => (true, "application/json"),
    None if accept.contains("application/yaml") => (false, "application/yaml; charset=utf-8"),
    None if accept.is_empty() || accept.contains("*/*") || accept.contains("application/vnd.oai.openapi") => {
      (false, "application/vnd.oai.openapi; charset=utf-8")
    }
    None => {
      let detail = "Could not satisfy the request Accept header.";
      return yaml_error(StatusCode::NOT_ACCEPTABLE, detail, "not_acceptable");
    }
  };
  let (body, filename) = if json {
    (include_str!("rest/schema/Bugsink.json"), "Bugsink.json")
  } else {
    (include_str!("rest/schema/Bugsink.yaml"), "Bugsink.yaml")
  };
  let disposition = format!("inline; filename=\"{filename}\"");
  let mut response = (StatusCode::OK, [(header::CONTENT_TYPE, content_type)], body).into_response();
  response.headers_mut().insert(
    header::CONTENT_DISPOSITION,
    HeaderValue::from_str(&disposition).expect("ascii"),
  );
  response
    .headers_mut()
    .insert(header::ALLOW, HeaderValue::from_static("GET, HEAD, OPTIONS"));
  response
}

/// Swagger UI on the schema (its scripts come from a CDN, as upstream's).
async fn swagger_ui() -> Response {
  let html = include_str!("rest/schema/swagger-ui.html");
  (
    [
      (header::CONTENT_TYPE, "text/html; charset=utf-8"),
      (header::ALLOW, "GET, HEAD, OPTIONS"),
    ],
    html,
  )
    .into_response()
}

// --- what a token may reach ---------------------------------------------------------------------------------------

/// `enforce_project_boundness`.
pub fn enforce_project_boundness(token: &Token, project_id: i64) -> Result<(), Drf> {
  match token.project_id {
    Some(bound) if bound != project_id => Err(Drf::forbidden("This token is not allowed to access this project.")),
    _ => Ok(()),
  }
}

/// `enforce_user_boundness_for_project`: what the token's user may do in the project.
pub async fn enforce_user_boundness_for_project(c: &Ctx, capability: &str, project_id: i64) -> Result<(), Drf> {
  let Some(user) = &c.token.user else { return Ok(()) };
  let pool = c.pool();
  match capability {
    "issues:read" | "events:read" | "issues:comment" | "issues:triage" | "issues:delete" | "releases:read" => {
      if !crate::memberships::has_project_access(pool, user, project_id).await? {
        return Err(Drf::forbidden(
          "The user bound to this token is not an accepted member of this project.",
        ));
      }
    }
    "releases:create" | "debug-files:upload" | "projects:manage" => {
      if !crate::memberships::is_project_admin(pool, user, project_id).await? {
        return Err(Drf::forbidden(
          "The user bound to this token does not administer this project.",
        ));
      }
    }
    "projects:read" => {
      if !project_visible(pool, user, project_id).await? {
        return Err(Drf::forbidden(
          "This project is not visible to the user bound to this token.",
        ));
      }
    }
    other => return Err(Drf::internal(format!("no project authorization rule for {other}"))),
  }
  Ok(())
}

/// `enforce_user_boundness_for_team`.
pub async fn enforce_user_boundness_for_team(c: &Ctx, capability: &str, team_id: Uuid) -> Result<(), Drf> {
  let Some(user) = &c.token.user else { return Ok(()) };
  if capability == "teams:read" {
    if !team_visible(c.pool(), user, team_id).await? {
      return Err(Drf::forbidden(
        "This team is not visible to the user bound to this token.",
      ));
    }
  } else if !crate::memberships::is_team_admin(c.pool(), user, Some(team_id)).await? {
    return Err(Drf::forbidden(
      "The user bound to this token does not administer this team.",
    ));
  }
  Ok(())
}

/// Both checks for a project a request is about.
pub async fn guard_project(c: &Ctx, capability: &str, project_id: i64) -> Result<(), Drf> {
  enforce_project_boundness(&c.token, project_id)?;
  enforce_user_boundness_for_project(c, capability, project_id).await
}

/// `projects_visible_to_user`, as SQL on `projects_project p` ($1: the user's id).
pub const PROJECT_VISIBLE: &str = "NOT p.is_deleted AND (
    EXISTS (SELECT 1 FROM projects_projectmembership pm WHERE pm.project_id = p.id AND pm.user_id = $1)
    OR EXISTS (SELECT 1 FROM teams_teammembership tm WHERE tm.team_id = p.team_id AND tm.user_id = $1 AND tm.accepted)
    OR p.visibility < 99)";

async fn project_visible(pool: &bugsink_db::PgPool, user: &User, project_id: i64) -> sqlx::Result<bool> {
  // not deleted, even for superusers
  let sql = format!(
    "SELECT EXISTS (SELECT 1 FROM projects_project p
                        WHERE p.id = $2 AND NOT p.is_deleted AND ($3 OR {PROJECT_VISIBLE}))"
  );
  sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
    .bind(user.id)
    .bind(project_id)
    .bind(user.is_superuser)
    .fetch_one(pool)
    .await
}

/// `teams_visible_to_user`, as SQL on `teams_team t` ($1: the user's id).
pub const TEAM_VISIBLE: &str = "(
    EXISTS (SELECT 1 FROM teams_teammembership tm WHERE tm.team_id = t.id AND tm.user_id = $1)
    OR t.visibility < 99)";

async fn team_visible(pool: &bugsink_db::PgPool, user: &User, team_id: Uuid) -> sqlx::Result<bool> {
  let sql = format!("SELECT EXISTS (SELECT 1 FROM teams_team t WHERE t.id = $2 AND ($3 OR {TEAM_VISIBLE}))");
  sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
    .bind(user.id)
    .bind(team_id)
    .bind(user.is_superuser)
    .fetch_one(pool)
    .await
}

// --- looking things up by what the request says -------------------------------------------------------------------

/// `get_object_or_404(Project.objects.all(), pk=...)` (deleted projects too unless `include_deleted` is false), if the
/// token may use it for `capability`.
pub async fn guarded_project(c: &Ctx, identifier: &str, include_deleted: bool, capability: &str) -> Result<i64, Drf> {
  let id = match py_int(identifier) {
    Some(PyInt::Small(id)) => id,
    Some(PyInt::Big(_)) => return Err(Drf::no_match("Project")),
    None => return Err(Drf::not_found()),
  };
  let exists: bool =
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM projects_project WHERE id = $1 AND ($2 OR NOT is_deleted))")
      .bind(id)
      .bind(include_deleted)
      .fetch_one(c.pool())
      .await?;
  if !exists {
    return Err(Drf::no_match("Project"));
  }
  guard_project(c, capability, id).await?;
  Ok(id)
}

/// A UUID primary key from the URL or the query; malformed: "Not found.".
pub fn uuid_pk(identifier: &str) -> Result<Uuid, Drf> {
  py_uuid(identifier).ok_or_else(Drf::not_found)
}

/// What upstream's `issue_lookup_kwargs` makes of an issue identifier: a UUID, or `SLUG-NUMBER` (the friendly id).
pub enum IssueKey {
  Id(Uuid),
  Friendly(String, i64),
}

pub fn issue_key(identifier: &str) -> Option<IssueKey> {
  if let Some(id) = py_uuid(identifier) {
    return Some(IssueKey::Id(id));
  }
  let (slug, number) = identifier.rsplit_once('-')?;
  match py_int(number)? {
    PyInt::Small(n) => Some(IssueKey::Friendly(slug.to_owned(), n)),
    PyInt::Big(_) => Some(IssueKey::Friendly(slug.to_owned(), i64::MAX)),
  }
}

/// "Invalid issue identifier.": a `ValidationError` without a field, so a list.
pub fn invalid_issue_identifier() -> Drf {
  Drf::invalid(json!(["Invalid issue identifier."]))
}

/// A (not deleted) issue by key: (id, project id).
pub async fn find_issue(c: &Ctx, key: &IssueKey) -> sqlx::Result<Option<(Uuid, i64)>> {
  match key {
    IssueKey::Id(id) => {
      sqlx::query_as("SELECT id, project_id FROM issues_issue WHERE id = $1 AND NOT is_deleted")
        .bind(id)
        .fetch_optional(c.pool())
        .await
    }
    IssueKey::Friendly(slug, number) => {
      sqlx::query_as(
        "SELECT i.id, i.project_id FROM issues_issue i JOIN projects_project p ON p.id = i.project_id
                 WHERE UPPER(p.slug::text) = UPPER($1) AND i.digest_order = $2 AND NOT i.is_deleted",
      )
      .bind(slug)
      .bind(i32::try_from(*number).unwrap_or(-1))
      .fetch_optional(c.pool())
      .await
    }
  }
}

/// The issue of a URL or query, if the token may use it for `capability`: (id, project id).
pub async fn guarded_issue(c: &Ctx, identifier: &str, capability: &str) -> Result<(Uuid, i64), Drf> {
  let key = issue_key(identifier).ok_or_else(invalid_issue_identifier)?;
  let (id, project_id) = find_issue(c, &key).await?.ok_or_else(|| Drf::no_match("Issue"))?;
  guard_project(c, capability, project_id).await?;
  Ok((id, project_id))
}

/// A required query parameter (a `ValidationError` on it when missing or empty).
pub fn required_param<'a>(req: &'a Request, key: &str) -> Result<&'a str, Drf> {
  match req.query.get(key) {
    None | Some("") => Err(Drf::invalid(json!({ key: [REQUIRED] }))),
    Some(v) => Ok(v),
  }
}

/// Whose hourly event counts.
pub enum CountsOf {
  Issue(Uuid),
  Project(i64),
}

/// `event_counts_response`: hourly counts (by digest time) between `?start` and `?end` (default: the last 24 hours).
pub async fn event_counts(c: &Ctx, of: CountsOf) -> Reply {
  use chrono::{
    DurationRound,
    TimeDelta,
    Utc,
  };
  let now = Utc::now();
  let current_hour = now.duration_trunc(TimeDelta::hours(1)).map_err(Drf::internal)?;
  let mut errors = fields::Errors::default();
  let mut parse = |key: &str| -> Option<chrono::DateTime<Utc>> {
    match c.req.query.get(key) {
      None | Some("") => None,
      Some(v) => errors.take(key, fields::datetime_field(&json!(v))),
    }
  };
  let (start, end) = (parse("start"), parse("end"));
  if !errors.is_empty() {
    return Err(Drf::invalid(errors.into_value()));
  }
  let end = end.unwrap_or(current_hour + TimeDelta::hours(1));
  let start = start.unwrap_or(end - TimeDelta::hours(24));
  let max_age = TimeDelta::days(90);
  let field_error = |field: &str, message: &str| Err(Drf::invalid(json!({ field: [message] })));
  for (name, value) in [("start", start), ("end", end)] {
    if value.duration_trunc(TimeDelta::hours(1)).ok() != Some(value) {
      return field_error(name, "Must be on a UTC hour boundary.");
    }
  }
  if start >= end {
    return field_error("end", "Must be after start.");
  }
  if end - start > max_age {
    return field_error("start", "The range cannot exceed 90 days.");
  }
  if start < current_hour - max_age {
    return field_error("start", "Hourly counts are retained for 90 days.");
  }
  if end > current_hour + TimeDelta::hours(1) {
    return field_error("end", "Cannot be later than the end of the current hour.");
  }
  let rows: Vec<(chrono::DateTime<Utc>, i32)> = match of {
    CountsOf::Issue(id) => {
      sqlx::query_as(
        "SELECT bucket, count FROM events_issueeventcountsperhour
             WHERE issue_id = $1 AND bucket >= $2 AND bucket < $3",
      )
      .bind(id)
    }
    CountsOf::Project(id) => {
      sqlx::query_as(
        "SELECT bucket, count FROM events_projecteventcountsperhour
             WHERE project_id = $1 AND bucket >= $2 AND bucket < $3",
      )
      .bind(id)
    }
  }
  .bind(start)
  .bind(end)
  .fetch_all(c.pool())
  .await?;
  let counts: std::collections::HashMap<_, _> = rows.into_iter().collect();
  let mut buckets = Vec::new();
  let mut t = start;
  while t < end {
    buckets.push(json!({"timestamp": drf_datetime(&t), "count": counts.get(&t).copied().unwrap_or(0)}));
    t += TimeDelta::hours(1);
  }
  ok(json!({
      "start": drf_datetime(&start),
      "end": drf_datetime(&end),
      "interval_seconds": 3600,
      "buckets": buckets,
  }))
}

/// `body_object` on `request.data`, as a `ValidationError`.
pub fn object_body(data: &Value) -> Result<&serde_json::Map<String, Value>, Drf> {
  fields::body_object(data).map_err(Drf::invalid)
}
