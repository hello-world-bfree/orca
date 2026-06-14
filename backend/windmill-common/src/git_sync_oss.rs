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
