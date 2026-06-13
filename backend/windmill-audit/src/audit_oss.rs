use windmill_common::{
    audit::AuditAuthor,
    db::{Authable, DbWithOptAuthed},
};

#[cfg(feature = "private")]
#[allow(unused)]
pub use crate::audit_ee::*;

/*
 * Author: Ruben Fiszel
 * Copyright: Windmill Labs, Inc 2022
 * This file and its contents are licensed under the AGPLv3 License.
 * Please see the included NOTICE for copyright information and
 * LICENSE-AGPL for a copy of the license.
 */
#[cfg(not(feature = "private"))]
use {
    crate::{ActionKind, AuditLog, ListAuditLogQuery},
    sqlx::{Postgres, Transaction},
    std::collections::HashMap,
    windmill_common::{
        error::{Error, Result},
        utils::Pagination,
    },
};

impl<'a, T: Authable + AuditAuthorable + Sync> AuditAuthorable for DbWithOptAuthed<'a, T> {
    fn email(&self) -> &str {
        match self {
            DbWithOptAuthed::UserDB { authed, .. } => AuditAuthorable::email(*authed),
            DbWithOptAuthed::DB { audit_author, .. } => audit_author.email(),
        }
    }
    fn username(&self) -> &str {
        match self {
            DbWithOptAuthed::UserDB { authed, .. } => AuditAuthorable::username(*authed),
            DbWithOptAuthed::DB { audit_author, .. } => audit_author.username(),
        }
    }
    fn username_override(&self) -> Option<&str> {
        match self {
            DbWithOptAuthed::UserDB { authed, .. } => AuditAuthorable::username_override(*authed),
            DbWithOptAuthed::DB { .. } => None,
        }
    }
}

impl AuditAuthorable for AuditAuthor {
    fn email(&self) -> &str {
        &self.email
    }

    fn username(&self) -> &str {
        &self.username
    }

    fn username_override(&self) -> Option<&str> {
        self.username_override.as_deref()
    }

    fn token_prefix(&self) -> Option<&str> {
        self.token_prefix.as_deref()
    }
}

pub trait AuditAuthorable {
    fn username(&self) -> &str;
    fn email(&self) -> &str;
    fn username_override(&self) -> Option<&str>;
    fn token_prefix(&self) -> Option<&str> {
        None
    }
}

// deviation: OSS audit-log implementation. Upstream ships these as no-op/empty stubs (audit logs are
// an EE feature). We write our own against the existing OSS-managed `audit` / `audit_partitioned`
// tables (schema + partition management already run in OSS via monitor.rs). New rows go to
// `audit_partitioned`; reads UNION ALL both tables. Row-level visibility is enforced by the existing
// RLS policies through the executor's role (UserDB), not by this code.
#[cfg(not(feature = "private"))]
const AUDIT_COLS: &str =
    "workspace_id, id, timestamp, username, operation, action_kind, resource, parameters, span";

#[cfg(not(feature = "private"))]
fn truncate(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

#[cfg(not(feature = "private"))]
#[tracing::instrument(level = "trace", skip_all)]
pub async fn audit_log<'c, E: sqlx::Executor<'c, Database = Postgres>>(
    db: E,
    author: &impl AuditAuthorable,
    operation: &str,
    action_kind: ActionKind,
    w_id: &str,
    resource: Option<&str>,
    parameters: Option<HashMap<&str, &str>>,
) -> Result<()> {
    let username = author
        .username_override()
        .unwrap_or_else(|| author.username());
    let parameters = parameters.map(|p| serde_json::json!(p));

    sqlx::query(
        "INSERT INTO audit_partitioned \
         (workspace_id, username, operation, action_kind, resource, parameters, email) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(w_id)
    .bind(truncate(username, 255))
    .bind(truncate(operation, 50))
    .bind(action_kind)
    .bind(resource.map(|r| truncate(r, 255)))
    .bind(parameters)
    .bind(truncate(author.email(), 255))
    .execute(db)
    .await?;
    Ok(())
}

#[cfg(not(feature = "private"))]
pub async fn list_audit(
    mut tx: Transaction<'_, Postgres>,
    w_id: String,
    pagination: Pagination,
    lq: ListAuditLogQuery,
) -> Result<Vec<AuditLog>> {
    use sql_builder::prelude::*;

    let (per_page, offset) = windmill_common::utils::paginate(pagination);

    let from = format!(
        "(SELECT {AUDIT_COLS} FROM audit UNION ALL SELECT {AUDIT_COLS} FROM audit_partitioned) AS t"
    );
    let mut sqlb = SqlBuilder::select_from(&from);
    sqlb.field(AUDIT_COLS)
        .order_by("timestamp", true)
        .order_by("id", true)
        .limit(per_page)
        .offset(offset);

    // "admins" workspace + all_workspaces lets a super admin read across workspaces; otherwise scope
    // to the requested workspace. Per-user vs. admin visibility is handled by RLS on the base tables.
    if !(w_id == "admins" && lq.all_workspaces.unwrap_or(false)) {
        sqlb.and_where_eq("workspace_id", "?".bind(&w_id));
    }
    if let Some(username) = &lq.username {
        sqlb.and_where_eq("username", "?".bind(username));
    }
    if let Some(operation) = &lq.operation {
        sqlb.and_where_eq("operation", "?".bind(operation));
    }
    if let Some(operations) = &lq.operations {
        let ops: Vec<String> = operations
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(quote)
            .collect();
        if !ops.is_empty() {
            sqlb.and_where_in("operation", &ops);
        }
    }
    if let Some(exclude) = &lq.exclude_operations {
        let ops: Vec<String> = exclude
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(quote)
            .collect();
        if !ops.is_empty() {
            sqlb.and_where(format!("operation NOT IN ({})", ops.join(", ")));
        }
    }
    if let Some(action_kind) = &lq.action_kind {
        sqlb.and_where_eq("action_kind::text", "?".bind(action_kind));
    }
    if let Some(resource) = &lq.resource {
        sqlb.and_where_eq("resource", "?".bind(resource));
    }
    if let Some(before) = &lq.before {
        sqlb.and_where_le("timestamp", "?".bind(&before.to_rfc3339()));
    }
    if let Some(after) = &lq.after {
        sqlb.and_where_ge("timestamp", "?".bind(&after.to_rfc3339()));
    }

    let sql = sqlb.sql().map_err(|e| Error::internal_err(e.to_string()))?;
    let rows = sqlx::query_as::<_, AuditLog>(&sql)
        .fetch_all(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(rows)
}

#[cfg(not(feature = "private"))]
pub async fn get_audit(mut tx: Transaction<'_, Postgres>, id: i32, w_id: &str) -> Result<AuditLog> {
    let sql = format!(
        "SELECT {AUDIT_COLS} FROM \
         (SELECT {AUDIT_COLS} FROM audit WHERE id = $1 AND workspace_id = $2 \
          UNION ALL \
          SELECT {AUDIT_COLS} FROM audit_partitioned WHERE id = $1 AND workspace_id = $2) AS t \
         LIMIT 1"
    );
    let audit = sqlx::query_as::<_, AuditLog>(&sql)
        .bind(id)
        .bind(w_id)
        .fetch_optional(&mut *tx)
        .await?;
    tx.commit().await?;
    audit.ok_or_else(|| Error::NotFound(format!("Audit log {id} not found in workspace {w_id}")))
}
