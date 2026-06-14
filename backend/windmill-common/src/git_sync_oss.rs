#[cfg(feature = "private")]
#[allow(unused)]
pub use crate::git_sync_ee::*;
#[cfg(not(feature = "private"))]
use sqlx::{Pool, Postgres, Row};
use url::Url;

#[cfg(not(feature = "private"))]
use crate::{
    auth::JWTAuthClaims,
    error::{to_anyhow, Error},
    global_settings::{load_value_from_global_settings, GITHUB_ENTERPRISE_APP_SETTING},
    jwt::decode_with_internal_secret,
};
#[cfg(not(feature = "private"))]
use serde::{Deserialize, Serialize};

// OSS deviation: our own AGPL implementation of the GitHub App git-sync auth that upstream
// ships only under `private` (`git_sync_ee.rs`). Standard GitHub App flow: sign an RS256
// app-JWT with the configured private key, exchange it for a short-lived installation token.
// No call into private code. See __docs/deviations-roadmap.md §Planned Task 7.

#[cfg(not(feature = "private"))]
struct GithubAppConfig {
    app_id: String,
    private_key: String,
    base_url: Option<String>,
}

#[cfg(not(feature = "private"))]
#[derive(Deserialize)]
struct InstallationRow {
    installation_id: i64,
    account_id: String,
    #[serde(default)]
    github_base_url: Option<String>,
    #[serde(default)]
    provisioned_by_admin: Option<bool>,
}

// Mirrors the OpenAPI `GithubInstallations` item + GHES config response (see openapi.yaml).
#[cfg(not(feature = "private"))]
#[derive(Serialize)]
pub struct GithubRepository {
    pub name: String,
    pub url: String,
}

#[cfg(not(feature = "private"))]
#[derive(Serialize)]
pub struct GithubInstallationInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    pub installation_id: i64,
    pub account_id: String,
    pub repositories: Vec<GithubRepository>,
    pub total_count: i64,
    pub per_page: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub github_base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provisioned_by_admin: Option<bool>,
}

#[cfg(not(feature = "private"))]
#[derive(Serialize)]
pub struct GhesConfigPublic {
    pub base_url: String,
    pub app_slug: String,
    pub client_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_owner: Option<String>,
}

#[cfg(not(feature = "private"))]
#[derive(Serialize)]
struct AppJwtClaims {
    iat: u64,
    exp: u64,
    iss: String,
}

#[cfg(not(feature = "private"))]
fn github_api_base(base_url: Option<&str>) -> String {
    match base_url {
        None => "https://api.github.com".to_string(),
        Some(b) => {
            let b = b.trim_end_matches('/');
            if b == "https://github.com" || b == "http://github.com" {
                "https://api.github.com".to_string()
            } else {
                format!("{b}/api/v3")
            }
        }
    }
}

#[cfg(not(feature = "private"))]
async fn load_github_app_config(db: &Pool<Postgres>) -> crate::error::Result<GithubAppConfig> {
    let value = load_value_from_global_settings(db, GITHUB_ENTERPRISE_APP_SETTING)
        .await?
        .ok_or_else(|| {
            Error::BadRequest(
                "GitHub App is not configured on this instance (instance settings → GitHub App)"
                    .to_string(),
            )
        })?;

    let app_id = value
        .get("app_id")
        .and_then(|x| match x {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Error::internal_err("GitHub App config missing `app_id`".to_string()))?;

    let private_key = value
        .get("private_key")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            Error::internal_err("GitHub App config missing `private_key`".to_string())
        })?;

    let base_url = value
        .get("base_url")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());

    Ok(GithubAppConfig { app_id, private_key, base_url })
}

#[cfg(not(feature = "private"))]
fn create_app_jwt(config: &GithubAppConfig) -> crate::error::Result<String> {
    let now = chrono::Utc::now().timestamp();
    let claims = AppJwtClaims {
        iat: (now - 60).max(0) as u64,
        exp: (now + 540).max(0) as u64,
        iss: config.app_id.clone(),
    };
    let key = jsonwebtoken::EncodingKey::from_rsa_pem(config.private_key.as_bytes())
        .map_err(to_anyhow)?;
    let token = jsonwebtoken::encode(
        &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256),
        &claims,
        &key,
    )
    .map_err(to_anyhow)?;
    Ok(token)
}

#[cfg(not(feature = "private"))]
async fn mint_installation_token(
    config: &GithubAppConfig,
    installation_id: i64,
    base_url_override: Option<&str>,
) -> crate::error::Result<String> {
    let app_jwt = create_app_jwt(config)?;
    let api_base = github_api_base(base_url_override.or(config.base_url.as_deref()));

    let resp = reqwest::Client::new()
        .post(format!(
            "{api_base}/app/installations/{installation_id}/access_tokens"
        ))
        .header("Authorization", format!("Bearer {app_jwt}"))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "windmill")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .map_err(to_anyhow)?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(Error::internal_err(format!(
            "GitHub App installation-token exchange failed ({status}): {body}"
        )));
    }

    #[derive(Deserialize)]
    struct TokenResponse {
        token: String,
    }
    let parsed: TokenResponse = resp.json().await.map_err(to_anyhow)?;
    Ok(parsed.token)
}

#[cfg(not(feature = "private"))]
async fn workspace_installations(
    db: &Pool<Postgres>,
    w_id: &str,
) -> crate::error::Result<Vec<InstallationRow>> {
    let row =
        sqlx::query("SELECT git_app_installations FROM workspace_settings WHERE workspace_id = $1")
            .bind(w_id)
            .fetch_optional(db)
            .await?
            .ok_or_else(|| Error::NotFound(format!("workspace settings not found for {w_id}")))?;

    let value: serde_json::Value = row
        .try_get("git_app_installations")
        .unwrap_or_else(|_| serde_json::Value::Array(vec![]));

    let installations = serde_json::from_value(value).map_err(to_anyhow)?;
    Ok(installations)
}

#[cfg(not(feature = "private"))]
fn repo_owner(repo_url: &str) -> crate::error::Result<String> {
    let url = Url::parse(repo_url)?;
    url.path_segments()
        .and_then(|mut segs| segs.next())
        .map(|s| s.trim_end_matches(".git"))
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            Error::BadRequest(format!(
                "Could not parse repository owner from URL: {repo_url}"
            ))
        })
}

// Used by the worker git clone path: resolves the installation by matching the repository's
// owner to a workspace installation's `account_id`. Deterministic even with multiple installs.
#[cfg(not(feature = "private"))]
pub async fn get_github_app_installation_token(
    db: &Pool<Postgres>,
    w_id: &str,
    repo_url: &str,
) -> crate::error::Result<String> {
    let owner = repo_owner(repo_url)?;
    let installations = workspace_installations(db, w_id).await?;
    let installation = installations
        .into_iter()
        .find(|i| i.account_id.eq_ignore_ascii_case(&owner))
        .ok_or_else(|| {
            Error::BadRequest(format!(
                "No GitHub App installation found for org '{owner}' in this workspace. \
                 Install the GitHub App on that org and attach the repository."
            ))
        })?;
    let config = load_github_app_config(db).await?;
    mint_installation_token(
        &config,
        installation.installation_id,
        installation.github_base_url.as_deref(),
    )
    .await
}

// EE-compatible entry point (used by the `/github_app/token` route). Resolves the workspace
// from the job token. With a single workspace installation this is unambiguous; with multiple
// it fails loud (clone-time resolution uses `get_github_app_installation_token` by repo URL).
#[cfg(not(feature = "private"))]
pub async fn get_github_app_token_internal(
    db: &Pool<Postgres>,
    job_token: &str,
) -> crate::error::Result<String> {
    let claims: JWTAuthClaims = decode_with_internal_secret(job_token).await?;
    let w_id = claims
        .workspace_id
        .ok_or_else(|| Error::BadRequest("Job token is not scoped to a workspace".to_string()))?;

    let installations = workspace_installations(db, &w_id).await?;
    match installations.as_slice() {
        [] => Err(Error::BadRequest(
            "No GitHub App installation configured for this workspace".to_string(),
        )),
        [installation] => {
            let config = load_github_app_config(db).await?;
            mint_installation_token(&config, installation.installation_id, installation.github_base_url.as_deref())
                .await
        }
        _ => Err(Error::BadRequest(
            "Multiple GitHub App installations in this workspace; cannot disambiguate from the job \
             token alone (resolution by repository URL is used at clone time)."
                .to_string(),
        )),
    }
}

#[cfg(not(feature = "private"))]
async fn list_installation_repos(
    config: &GithubAppConfig,
    installation_id: i64,
    base_url_override: Option<&str>,
) -> crate::error::Result<(Vec<GithubRepository>, i64)> {
    let token = mint_installation_token(config, installation_id, base_url_override).await?;
    let api_base = github_api_base(base_url_override.or(config.base_url.as_deref()));

    let resp = reqwest::Client::new()
        .get(format!("{api_base}/installation/repositories?per_page=100"))
        .header("Authorization", format!("token {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "windmill")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .map_err(to_anyhow)?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(Error::internal_err(format!(
            "GitHub list-repositories failed ({status}): {body}"
        )));
    }

    #[derive(Deserialize)]
    struct Repo {
        full_name: String,
        clone_url: String,
    }
    #[derive(Deserialize)]
    struct ReposResponse {
        total_count: i64,
        repositories: Vec<Repo>,
    }
    let parsed: ReposResponse = resp.json().await.map_err(to_anyhow)?;
    let repositories = parsed
        .repositories
        .into_iter()
        .map(|r| GithubRepository { name: r.full_name, url: r.clone_url })
        .collect();
    Ok((repositories, parsed.total_count))
}

// Enumerates every workspace's stored installations and resolves each one's repositories via the
// GitHub API (best-effort: a per-installation failure is captured in `error`, not propagated).
#[cfg(not(feature = "private"))]
pub async fn list_all_connected_installations(
    db: &Pool<Postgres>,
) -> crate::error::Result<Vec<GithubInstallationInfo>> {
    let config = load_github_app_config(db).await?;

    let rows = sqlx::query(
        "SELECT workspace_id, git_app_installations FROM workspace_settings \
         WHERE git_app_installations IS NOT NULL AND git_app_installations <> '[]'::jsonb",
    )
    .fetch_all(db)
    .await?;

    let mut out = Vec::new();
    for row in rows {
        let workspace_id: String = row.try_get("workspace_id").unwrap_or_default();
        let value: serde_json::Value = row
            .try_get("git_app_installations")
            .unwrap_or_else(|_| serde_json::Value::Array(vec![]));
        let installations: Vec<InstallationRow> = serde_json::from_value(value).unwrap_or_default();

        for inst in installations {
            let (repositories, total_count, error) = match list_installation_repos(
                &config,
                inst.installation_id,
                inst.github_base_url.as_deref(),
            )
            .await
            {
                Ok((repos, total)) => (repos, total, None),
                Err(e) => (Vec::new(), 0, Some(e.to_string())),
            };
            out.push(GithubInstallationInfo {
                workspace_id: Some(workspace_id.clone()),
                installation_id: inst.installation_id,
                account_id: inst.account_id,
                repositories,
                total_count,
                per_page: 100,
                error,
                github_base_url: inst.github_base_url,
                provisioned_by_admin: inst.provisioned_by_admin,
            });
        }
    }
    Ok(out)
}

// Returns the public GitHub App config used to build the installation URL. None when the instance
// has no self-managed app configured (the frontend then treats it as the github.com cloud flow).
#[cfg(not(feature = "private"))]
pub async fn get_ghes_config(
    db: &Pool<Postgres>,
) -> crate::error::Result<Option<GhesConfigPublic>> {
    let value = match load_value_from_global_settings(db, GITHUB_ENTERPRISE_APP_SETTING).await? {
        Some(v) => v,
        None => return Ok(None),
    };
    let field = |key: &str| {
        value
            .get(key)
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
    };
    match (field("base_url"), field("app_slug"), field("client_id")) {
        (Some(base_url), Some(app_slug), Some(client_id)) => Ok(Some(GhesConfigPublic {
            base_url,
            app_slug,
            client_id,
            app_owner: field("app_owner"),
        })),
        _ => Ok(None),
    }
}

pub fn prepend_token_to_github_url(
    github_url: &str,
    installation_token: &str,
) -> crate::error::Result<String> {
    let url = Url::parse(github_url)?;

    let host = url.host_str().ok_or_else(|| {
        crate::error::Error::BadRequest("Invalid GitHub URL: no host".to_string())
    })?;

    Ok(format!(
        "https://x-access-token:{}@{}{}",
        installation_token,
        host,
        url.path()
    ))
}
