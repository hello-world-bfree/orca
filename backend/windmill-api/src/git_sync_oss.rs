#[cfg(feature = "private")]
#[allow(unused)]
pub use crate::git_sync_ee::*;

#[cfg(not(feature = "private"))]
use axum::{extract::Query, routing::get, Extension, Json, Router};
#[cfg(not(feature = "private"))]
use serde::Deserialize;

#[cfg(not(feature = "private"))]
use windmill_common::{
    error::{Error, JsonResult},
    git_sync_oss::{
        get_ghes_config, list_all_connected_installations, GhesConfigPublic, GithubInstallationInfo,
    },
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
