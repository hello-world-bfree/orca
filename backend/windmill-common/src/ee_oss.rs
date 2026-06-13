#[cfg(feature = "private")]
#[allow(unused)]
pub use crate::ee::*;

#[cfg(all(feature = "enterprise", not(feature = "private")))]
use crate::db::DB;
#[cfg(not(feature = "private"))]
use crate::ee_oss::LicensePlan::Community;
#[cfg(all(feature = "enterprise", not(feature = "private")))]
use crate::error;
#[cfg(not(feature = "private"))]
use serde::Deserialize;
#[cfg(not(feature = "private"))]
use std::sync::atomic::AtomicBool;

#[cfg(not(feature = "private"))]
lazy_static::lazy_static! {
  pub static ref LICENSE_KEY_VALID: AtomicBool = AtomicBool::new(true);
  pub static ref LICENSE_KEY_ID: arc_swap::ArcSwap<String> = arc_swap::ArcSwap::from_pointee("".to_string());
  pub static ref LICENSE_KEY: arc_swap::ArcSwap<String> = arc_swap::ArcSwap::from_pointee("".to_string());
  pub static ref LICENSE_OFFLINE_METADATA: arc_swap::ArcSwap<Option<OfflineMetadata>> = arc_swap::ArcSwap::from_pointee(None);
  pub static ref LICENSE_OFFLINE_OVER_CU_CAP: AtomicBool = AtomicBool::new(false);
  pub static ref LICENSE_OFFLINE_LAST_STATUS: arc_swap::ArcSwap<Option<OfflineCapStatus>> = arc_swap::ArcSwap::from_pointee(None);
  pub static ref LICENSE_OFFLINE_LAST_CHECKED_AT: arc_swap::ArcSwap<Option<chrono::DateTime<chrono::Utc>>> = arc_swap::ArcSwap::from_pointee(None);
}

#[cfg(not(feature = "private"))]
#[derive(Clone, Debug, Deserialize, serde::Serialize)]
pub struct OfflineMetadata {
    pub v: u32,
    pub kind: String,
    pub hash: String,
    pub seats: i64,
    pub cu_limit: f64,
}

#[cfg(not(feature = "private"))]
impl OfflineMetadata {
    pub fn is_offline(&self) -> bool {
        self.kind == "offline"
    }
}

#[cfg(not(feature = "private"))]
#[derive(Clone, Debug, serde::Serialize)]
pub struct OfflineCapStatus {
    pub seats_used: f64,
    pub seats_cap: i64,
    pub author_count: i64,
    pub operator_count: i64,
    pub current_cu: f64,
    pub cu_cap: f64,
    pub cu_over_cap: bool,
}

#[cfg(all(feature = "enterprise", not(feature = "private")))]
pub async fn check_seat_cap_for_new_user(
    _db: &DB,
    _email: &str,
    _new_user_is_operator: bool,
) -> anyhow::Result<Option<String>> {
    Ok(None)
}

#[cfg(all(feature = "enterprise", not(feature = "private")))]
pub async fn compute_instance_hash(_db: &DB) -> anyhow::Result<Option<String>> {
    // Implementation is not open source
    Ok(None)
}

#[cfg(all(feature = "enterprise", not(feature = "private")))]
pub async fn enforce_offline_caps(_db: &DB) -> anyhow::Result<Option<OfflineCapStatus>> {
    // Implementation is not open source
    Ok(None)
}

#[cfg(not(feature = "private"))]
#[derive(PartialEq, Eq)]
pub enum LicensePlan {
    Community,
    Pro,
    Enterprise,
}

#[cfg(not(feature = "private"))]
pub async fn get_license_plan() -> LicensePlan {
    // Implementation is not open source
    return Community;
}

#[derive(Deserialize)]
#[serde(untagged)]
#[cfg(not(feature = "private"))]
pub enum CriticalErrorChannel {
    Email { email: String },
    Slack { slack_channel: String },
    Teams { teams_channel: TeamsChannel },
}

#[derive(Deserialize)]
#[cfg(not(feature = "private"))]
pub struct TeamsChannel {
    pub team_id: String,
    pub team_name: String,
    pub channel_id: String,
    pub channel_name: String,
}

#[cfg(not(feature = "private"))]
pub enum CriticalAlertKind {
    CriticalError,
    RecoveredCriticalError,
}

#[cfg(all(feature = "enterprise", not(feature = "private")))]
pub async fn send_critical_alert(
    _error_message: String,
    _db: &DB,
    _kind: CriticalAlertKind,
    _channels: Option<Vec<CriticalErrorChannel>>,
) {
}

// deviation: OSS critical-alert delivery (Email + Slack). Upstream gates delivery behind
// `enterprise`; we provide an OSS implementation that iterates the configured channels.
#[cfg(all(not(feature = "enterprise"), not(feature = "private")))]
pub async fn send_critical_alert(
    error_message: String,
    db: &crate::db::DB,
    kind: CriticalAlertKind,
    channels: Option<Vec<CriticalErrorChannel>>,
) {
    let subject = match kind {
        CriticalAlertKind::CriticalError => "Critical error on Windmill instance",
        CriticalAlertKind::RecoveredCriticalError => {
            "Recovered from critical error on Windmill instance"
        }
    };

    let global = crate::CRITICAL_ERROR_CHANNELS.load_full();
    let channels: &[CriticalErrorChannel] = match channels.as_deref() {
        Some(channels) => channels,
        None => &global,
    };

    for channel in channels {
        match channel {
            CriticalErrorChannel::Email { email } => {
                let smtp_cfg = crate::worker::SMTP_CONFIG.load_full();
                if let Some(smtp) = smtp_cfg.as_ref().as_ref() {
                    if let Err(e) = crate::email_oss::send_email_plain_text(
                        subject,
                        &error_message,
                        vec![email.clone()],
                        smtp.clone(),
                        None,
                    )
                    .await
                    {
                        tracing::error!("Failed to send critical alert email to {email}: {e}");
                    }
                } else {
                    tracing::warn!(
                        "SMTP not configured; cannot send critical alert email to {email}"
                    );
                }
            }
            CriticalErrorChannel::Slack { slack_channel } => {
                if let Err(e) =
                    send_critical_alert_slack(db, slack_channel, subject, &error_message).await
                {
                    tracing::error!(
                        "Failed to send critical alert to Slack channel {slack_channel}: {e}"
                    );
                }
            }
            CriticalErrorChannel::Teams { .. } => {
                tracing::warn!(
                    "Microsoft Teams critical alerts are not supported in this OSS build"
                );
            }
        }
    }
}

#[cfg(all(not(feature = "enterprise"), not(feature = "private")))]
async fn send_critical_alert_slack(
    db: &crate::db::DB,
    channel: &str,
    subject: &str,
    message: &str,
) -> crate::error::Result<()> {
    let token = crate::variables::get_secret_value_as_admin(
        db,
        "admins",
        crate::oauth2::GLOBAL_SLACK_BOT_TOKEN_PATH,
    )
    .await?;

    let resp = reqwest::Client::new()
        .post("https://slack.com/api/chat.postMessage")
        .bearer_auth(token)
        .json(&serde_json::json!({
            "channel": channel,
            "text": format!("*{subject}*\n{message}"),
        }))
        .send()
        .await
        .map_err(|e| crate::error::Error::internal_err(format!("Slack request failed: {e}")))?;

    let status = resp.status();
    let body: serde_json::Value = resp.json().await.map_err(|e| {
        crate::error::Error::internal_err(format!("Slack response parse failed: {e}"))
    })?;
    if !body.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
        return Err(crate::error::Error::internal_err(format!(
            "Slack API error (status {status}): {body}"
        )));
    }
    Ok(())
}

#[cfg(all(feature = "enterprise", not(feature = "private")))]
pub async fn maybe_renew_license_key_on_start(
    _http_client: &reqwest::Client,
    _db: &crate::db::DB,
    force_renew_now: bool,
) -> bool {
    // Implementation is not open source
    force_renew_now
}

#[cfg(all(feature = "enterprise", not(feature = "private")))]
pub enum RenewReason {
    Manual,
    Schedule,
    OnStart,
}

#[cfg(all(feature = "enterprise", not(feature = "private")))]
pub async fn renew_license_key(
    _http_client: &reqwest::Client,
    _db: &crate::db::DB,
    _key: Option<String>,
    _reason: RenewReason,
) -> String {
    // Implementation is not open source
    "".to_string()
}

#[cfg(all(feature = "enterprise", not(feature = "private")))]
pub async fn create_customer_portal_session(
    _http_client: &reqwest::Client,
    _key: Option<String>,
) -> error::Result<String> {
    // Implementation is not open source
    Ok("".to_string())
}

#[cfg(all(feature = "enterprise", not(feature = "private")))]
pub async fn worker_groups_alerts(_db: &DB) {}

#[cfg(all(feature = "enterprise", not(feature = "private")))]
pub async fn jobs_waiting_alerts(_db: &DB) {}

#[cfg(all(feature = "enterprise", not(feature = "private")))]
pub async fn low_disk_alerts(
    _db: &DB,
    _server_mode: bool,
    _worker_mode: bool,
    _workers: Vec<String>,
) {
    // Implementation is not open source
}
