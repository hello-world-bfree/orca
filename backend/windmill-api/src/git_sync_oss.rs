#[cfg(feature = "private")]
#[allow(unused)]
pub use crate::git_sync_ee::*;

#[cfg(not(feature = "private"))]
use axum::{
    extract::{Path, Query},
    routing::{delete, get, post},
    Extension, Json, Router,
};
#[cfg(not(feature = "private"))]
use serde::{Deserialize, Serialize};

#[cfg(not(feature = "private"))]
use windmill_common::{
    error::{Error, JsonResult, Result},
    git_sync_oss::{
        copy_installation_to_workspace, get_ghes_config, get_github_app_token_internal,
        list_all_connected_installations, register_installation,
        remove_installation_from_workspace, GhesConfigPublic, GithubInstallationInfo,
    },
    utils::require_admin,
};

#[cfg(not(feature = "private"))]
use crate::db::{ApiAuthed, DB};
#[cfg(not(feature = "private"))]
use crate::utils::require_super_admin;

// OSS deviation: our own AGPL implementation of the GitHub App git-sync routes (upstream ships
// these only under `private`). See __docs/deviations-roadmap.md §Planned Task 7. Phase B1 covers
// the read path (`connected_repositories`, `ghes_config`); workspace CRUD + GHES admin land next.

#[cfg(not(feature = "private"))]
pub fn workspaced_service() -> Router {
    Router::new()
        .route("/token", post(exchange_github_app_token))
        .route("/install_from_workspace", post(install_from_workspace))
        .route(
            "/installation/{installation_id}",
            delete(delete_installation),
        )
        .route(
            "/ghes_installation_callback",
            post(ghes_installation_callback),
        )
}

#[cfg(not(feature = "private"))]
#[derive(Deserialize)]
struct TokenRequest {
    job_token: String,
}

#[cfg(not(feature = "private"))]
#[derive(Serialize)]
struct TokenResponse {
    token: String,
}

// Called by a running git-sync job to exchange its job token for a GitHub App installation token.
// Authed as a workspace member; the installation is resolved from the job token itself.
#[cfg(not(feature = "private"))]
async fn exchange_github_app_token(
    _authed: ApiAuthed,
    Extension(db): Extension<DB>,
    Path(_w_id): Path<String>,
    Json(req): Json<TokenRequest>,
) -> JsonResult<TokenResponse> {
    let token = get_github_app_token_internal(&db, &req.job_token).await?;
    Ok(Json(TokenResponse { token }))
}

#[cfg(not(feature = "private"))]
#[derive(Deserialize)]
struct InstallFromWorkspaceRequest {
    source_workspace_id: String,
    installation_id: i64,
}

#[cfg(not(feature = "private"))]
async fn install_from_workspace(
    authed: ApiAuthed,
    Extension(db): Extension<DB>,
    Path(w_id): Path<String>,
    Json(req): Json<InstallFromWorkspaceRequest>,
) -> Result<String> {
    require_admin(authed.is_admin, &authed.username)?;
    copy_installation_to_workspace(&db, &req.source_workspace_id, &w_id, req.installation_id)
        .await?;
    Ok("installation copied to workspace".to_string())
}

#[cfg(not(feature = "private"))]
async fn delete_installation(
    authed: ApiAuthed,
    Extension(db): Extension<DB>,
    Path((w_id, installation_id)): Path<(String, i64)>,
) -> Result<String> {
    require_admin(authed.is_admin, &authed.username)?;
    let is_super_admin = require_super_admin(&db, &authed.email).await.is_ok();
    remove_installation_from_workspace(&db, &w_id, installation_id, is_super_admin).await?;
    Ok("installation removed from workspace".to_string())
}

#[cfg(not(feature = "private"))]
#[derive(Deserialize)]
struct GhesCallbackRequest {
    installation_id: i64,
}

#[cfg(not(feature = "private"))]
async fn ghes_installation_callback(
    authed: ApiAuthed,
    Extension(db): Extension<DB>,
    Path(w_id): Path<String>,
    Json(req): Json<GhesCallbackRequest>,
) -> Result<String> {
    require_admin(authed.is_admin, &authed.username)?;
    register_installation(&db, &w_id, req.installation_id).await?;
    Ok("installation registered".to_string())
}

#[cfg(not(feature = "private"))]
pub fn global_service() -> Router {
    Router::new()
        .route("/connected_repositories", get(get_connected_repositories))
        .route("/ghes_config", get(get_ghes_config_handler))
}

#[cfg(not(feature = "private"))]
#[derive(Deserialize)]
struct PageQuery {
    #[allow(dead_code)]
    page: Option<i64>,
}

#[cfg(not(feature = "private"))]
async fn get_connected_repositories(
    ApiAuthed { email, .. }: ApiAuthed,
    Extension(db): Extension<DB>,
    Query(_page): Query<PageQuery>,
) -> JsonResult<Vec<GithubInstallationInfo>> {
    require_super_admin(&db, &email).await?;
    Ok(Json(list_all_connected_installations(&db).await?))
}

#[cfg(not(feature = "private"))]
async fn get_ghes_config_handler(
    ApiAuthed { email, .. }: ApiAuthed,
    Extension(db): Extension<DB>,
) -> JsonResult<GhesConfigPublic> {
    require_super_admin(&db, &email).await?;
    let config = get_ghes_config(&db)
        .await?
        .ok_or_else(|| Error::NotFound("No self-managed GitHub App configured".to_string()))?;
    Ok(Json(config))
}
