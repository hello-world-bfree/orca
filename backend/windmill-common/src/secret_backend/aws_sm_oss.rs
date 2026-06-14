/*
 * Author: Windmill Labs, Inc
 * Copyright (C) Windmill Labs, Inc - All Rights Reserved
 * Unauthorized copying of this file, via any medium is strictly prohibited.
 */

// OSS deviation: own AGPL implementation of the AWS Secrets Manager secret
// backend, gated on the `aws_secrets_manager` feature. Upstream ships the real
// impl only under `private` (aws_sm_ee.rs). When the feature is off this stays a
// stub so non-OSS builds keep the original behaviour and upstream merges stay clean.

use async_trait::async_trait;

use crate::db::DB;
use crate::error::{Error, Result};

use super::{
    AwsSecretsManagerSettings, SecretBackend, SecretMigrationFailure, SecretMigrationReport,
};

#[cfg(feature = "aws_secrets_manager")]
use crate::variables::{build_crypt, decrypt, encrypt};

#[cfg(feature = "aws_secrets_manager")]
pub struct AwsSecretsManagerBackend {
    client: aws_sdk_secretsmanager::Client,
    prefix: String,
}

#[cfg(feature = "aws_secrets_manager")]
async fn build_client(settings: &AwsSecretsManagerSettings) -> aws_sdk_secretsmanager::Client {
    use aws_sdk_secretsmanager::config::{Credentials, Region};

    let mut loader = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(Region::new(settings.region.clone()));

    if let (Some(access_key_id), Some(secret_access_key)) = (
        settings.access_key_id.as_ref(),
        settings.secret_access_key.as_ref(),
    ) {
        if !access_key_id.is_empty() && !secret_access_key.is_empty() {
            loader = loader.credentials_provider(Credentials::new(
                access_key_id.clone(),
                secret_access_key.clone(),
                None,
                None,
                "windmill-aws-sm",
            ));
        }
    }

    if let Some(endpoint_url) = settings.endpoint_url.as_ref() {
        if !endpoint_url.is_empty() {
            loader = loader.endpoint_url(endpoint_url.clone());
        }
    }

    let shared = loader.load().await;
    aws_sdk_secretsmanager::Client::new(&shared)
}

#[cfg(feature = "aws_secrets_manager")]
impl AwsSecretsManagerBackend {
    pub async fn new_with_client(settings: AwsSecretsManagerSettings) -> Result<Self> {
        let client = build_client(&settings).await;
        Ok(AwsSecretsManagerBackend { client, prefix: settings.prefix.unwrap_or_default() })
    }

    fn secret_name(&self, workspace_id: &str, path: &str) -> String {
        format!("{}{}/{}", self.prefix, workspace_id, path)
    }
}

#[cfg(feature = "aws_secrets_manager")]
#[async_trait]
impl SecretBackend for AwsSecretsManagerBackend {
    async fn get_secret(&self, workspace_id: &str, path: &str) -> Result<String> {
        let name = self.secret_name(workspace_id, path);
        match self.client.get_secret_value().secret_id(&name).send().await {
            Ok(output) => output
                .secret_string()
                .map(|s| s.to_string())
                .ok_or_else(|| {
                    Error::internal_err(format!("AWS secret {} has no string value", name))
                }),
            Err(err) => {
                let service_err = err.into_service_error();
                if service_err.is_resource_not_found_exception() {
                    Err(Error::NotFound(format!("secret {} not found", name)))
                } else {
                    Err(Error::internal_err(format!(
                        "AWS Secrets Manager get failed for {}: {}",
                        name, service_err
                    )))
                }
            }
        }
    }

    async fn set_secret(&self, workspace_id: &str, path: &str, value: &str) -> Result<()> {
        let name = self.secret_name(workspace_id, path);
        match self
            .client
            .create_secret()
            .name(&name)
            .secret_string(value)
            .send()
            .await
        {
            Ok(_) => Ok(()),
            Err(err) => {
                let service_err = err.into_service_error();
                if service_err.is_resource_exists_exception() {
                    self.client
                        .put_secret_value()
                        .secret_id(&name)
                        .secret_string(value)
                        .send()
                        .await
                        .map_err(|e| {
                            Error::internal_err(format!(
                                "AWS Secrets Manager update failed for {}: {}",
                                name,
                                e.into_service_error()
                            ))
                        })?;
                    Ok(())
                } else {
                    Err(Error::internal_err(format!(
                        "AWS Secrets Manager create failed for {}: {}",
                        name, service_err
                    )))
                }
            }
        }
    }

    async fn delete_secret(&self, workspace_id: &str, path: &str) -> Result<()> {
        let name = self.secret_name(workspace_id, path);
        match self
            .client
            .delete_secret()
            .secret_id(&name)
            .force_delete_without_recovery(true)
            .send()
            .await
        {
            Ok(_) => Ok(()),
            Err(err) => {
                let service_err = err.into_service_error();
                if service_err.is_resource_not_found_exception() {
                    Ok(())
                } else {
                    Err(Error::internal_err(format!(
                        "AWS Secrets Manager delete failed for {}: {}",
                        name, service_err
                    )))
                }
            }
        }
    }

    fn backend_name(&self) -> &'static str {
        "aws_secrets_manager"
    }
}

#[cfg(feature = "aws_secrets_manager")]
pub async fn test_aws_sm_connection(settings: &AwsSecretsManagerSettings) -> Result<()> {
    let client = build_client(settings).await;
    client
        .list_secrets()
        .max_results(1)
        .send()
        .await
        .map_err(|e| {
            Error::internal_err(format!(
                "AWS Secrets Manager connection test failed: {}",
                e.into_service_error()
            ))
        })?;
    Ok(())
}

#[cfg(feature = "aws_secrets_manager")]
fn is_external_marker(value: &str) -> bool {
    value.starts_with("$vault:") || value.starts_with("$azure_kv:") || value.starts_with("$aws_sm:")
}

#[cfg(feature = "aws_secrets_manager")]
pub async fn migrate_secrets_to_aws_sm(
    db: &DB,
    settings: &AwsSecretsManagerSettings,
) -> Result<SecretMigrationReport> {
    let backend = AwsSecretsManagerBackend::new_with_client(settings.clone()).await?;

    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT workspace_id, path, value FROM variable WHERE is_secret IS TRUE AND value != ''",
    )
    .fetch_all(db)
    .await?;

    let mut report = SecretMigrationReport {
        total_secrets: rows.len(),
        migrated_count: 0,
        failed_count: 0,
        failures: vec![],
    };

    for (workspace_id, path, value) in rows {
        if is_external_marker(&value) {
            continue;
        }

        let migrate = async {
            let mc = build_crypt(db, &workspace_id).await?;
            let plain = decrypt(&mc, value.clone())?;
            backend.set_secret(&workspace_id, &path, &plain).await?;
            sqlx::query("UPDATE variable SET value = $1 WHERE workspace_id = $2 AND path = $3")
                .bind(format!("$aws_sm:{}", path))
                .bind(&workspace_id)
                .bind(&path)
                .execute(db)
                .await?;
            Ok::<(), Error>(())
        }
        .await;

        match migrate {
            Ok(()) => report.migrated_count += 1,
            Err(e) => {
                report.failed_count += 1;
                report.failures.push(SecretMigrationFailure {
                    workspace_id,
                    path,
                    error: e.to_string(),
                });
            }
        }
    }

    Ok(report)
}

#[cfg(feature = "aws_secrets_manager")]
pub async fn migrate_secrets_from_aws_sm(
    db: &DB,
    settings: &AwsSecretsManagerSettings,
) -> Result<SecretMigrationReport> {
    let backend = AwsSecretsManagerBackend::new_with_client(settings.clone()).await?;

    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT workspace_id, path, value FROM variable WHERE is_secret IS TRUE AND value LIKE '$aws_sm:%'",
    )
    .fetch_all(db)
    .await?;

    let mut report = SecretMigrationReport {
        total_secrets: rows.len(),
        migrated_count: 0,
        failed_count: 0,
        failures: vec![],
    };

    for (workspace_id, path, _value) in rows {
        let migrate = async {
            let plain = backend.get_secret(&workspace_id, &path).await?;
            let mc = build_crypt(db, &workspace_id).await?;
            let encrypted = encrypt(&mc, &plain);
            sqlx::query("UPDATE variable SET value = $1 WHERE workspace_id = $2 AND path = $3")
                .bind(encrypted)
                .bind(&workspace_id)
                .bind(&path)
                .execute(db)
                .await?;
            backend.delete_secret(&workspace_id, &path).await?;
            Ok::<(), Error>(())
        }
        .await;

        match migrate {
            Ok(()) => report.migrated_count += 1,
            Err(e) => {
                report.failed_count += 1;
                report.failures.push(SecretMigrationFailure {
                    workspace_id,
                    path,
                    error: e.to_string(),
                });
            }
        }
    }

    Ok(report)
}

#[cfg(not(feature = "aws_secrets_manager"))]
pub struct AwsSecretsManagerBackend;

#[cfg(not(feature = "aws_secrets_manager"))]
impl AwsSecretsManagerBackend {
    pub async fn new_with_client(_settings: AwsSecretsManagerSettings) -> Result<Self> {
        Err(Error::internal_err(
            "AWS Secrets Manager integration requires the 'aws_secrets_manager' feature"
                .to_string(),
        ))
    }
}

#[cfg(not(feature = "aws_secrets_manager"))]
#[async_trait]
impl SecretBackend for AwsSecretsManagerBackend {
    async fn get_secret(&self, _workspace_id: &str, _path: &str) -> Result<String> {
        Err(Error::internal_err(
            "AWS Secrets Manager integration requires the 'aws_secrets_manager' feature"
                .to_string(),
        ))
    }

    async fn set_secret(&self, _workspace_id: &str, _path: &str, _value: &str) -> Result<()> {
        Err(Error::internal_err(
            "AWS Secrets Manager integration requires the 'aws_secrets_manager' feature"
                .to_string(),
        ))
    }

    async fn delete_secret(&self, _workspace_id: &str, _path: &str) -> Result<()> {
        Err(Error::internal_err(
            "AWS Secrets Manager integration requires the 'aws_secrets_manager' feature"
                .to_string(),
        ))
    }

    fn backend_name(&self) -> &'static str {
        "aws_secrets_manager"
    }
}

#[cfg(not(feature = "aws_secrets_manager"))]
pub async fn test_aws_sm_connection(_settings: &AwsSecretsManagerSettings) -> Result<()> {
    Err(Error::internal_err(
        "AWS Secrets Manager integration requires the 'aws_secrets_manager' feature".to_string(),
    ))
}

#[cfg(not(feature = "aws_secrets_manager"))]
pub async fn migrate_secrets_to_aws_sm(
    _db: &DB,
    _settings: &AwsSecretsManagerSettings,
) -> Result<SecretMigrationReport> {
    Err(Error::internal_err(
        "AWS Secrets Manager integration requires the 'aws_secrets_manager' feature".to_string(),
    ))
}

#[cfg(not(feature = "aws_secrets_manager"))]
pub async fn migrate_secrets_from_aws_sm(
    _db: &DB,
    _settings: &AwsSecretsManagerSettings,
) -> Result<SecretMigrationReport> {
    Err(Error::internal_err(
        "AWS Secrets Manager integration requires the 'aws_secrets_manager' feature".to_string(),
    ))
}
