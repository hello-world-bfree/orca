#[cfg(feature = "private")]
#[allow(unused)]
pub use crate::email_ee::*;

#[cfg(not(feature = "private"))]
use crate::server::Smtp;

#[cfg(not(feature = "private"))]
enum EmailBody<'a> {
    Html(&'a str),
    Text(&'a str),
}

#[cfg(all(not(feature = "private"), feature = "smtp"))]
async fn deliver_email(
    subject: &str,
    body: EmailBody<'_>,
    to: Vec<String>,
    smtp: Smtp,
    client_timeout: Option<tokio::time::Duration>,
) -> crate::error::Result<()> {
    use crate::error::Error;
    use mail_send::mail_builder::MessageBuilder;
    use mail_send::SmtpClientBuilder;

    if to.is_empty() {
        return Ok(());
    }

    let message = MessageBuilder::new()
        .from(smtp.from.clone())
        .to(to)
        .subject(subject);
    let message = match body {
        EmailBody::Html(content) => message.html_body(content),
        EmailBody::Text(content) => message.text_body(content),
    };

    let mut builder = SmtpClientBuilder::new(smtp.host.clone(), smtp.port)
        .implicit_tls(smtp.tls_implicit.unwrap_or(false));
    if let Some(timeout) = client_timeout {
        builder = builder.timeout(timeout);
    }
    if let Some(username) = smtp.username.clone() {
        builder = builder.credentials((username, smtp.password.clone().unwrap_or_default()));
    }

    if smtp.disable_tls.unwrap_or(false) {
        builder
            .connect_plain()
            .await
            .map_err(|e| Error::internal_err(format!("SMTP connection failed: {e}")))?
            .send(message)
            .await
            .map_err(|e| Error::internal_err(format!("SMTP send failed: {e}")))?;
    } else {
        builder
            .connect()
            .await
            .map_err(|e| Error::internal_err(format!("SMTP connection failed: {e}")))?
            .send(message)
            .await
            .map_err(|e| Error::internal_err(format!("SMTP send failed: {e}")))?;
    }

    Ok(())
}

#[cfg(all(not(feature = "private"), not(feature = "smtp")))]
async fn deliver_email(
    _subject: &str,
    _body: EmailBody<'_>,
    _to: Vec<String>,
    _smtp: Smtp,
    _client_timeout: Option<tokio::time::Duration>,
) -> crate::error::Result<()> {
    tracing::warn!("Email not sent: the `smtp` feature is not enabled in this build");
    Ok(())
}

#[cfg(not(feature = "private"))]
pub async fn send_email(
    subject: &str,
    content: &str,
    to: Vec<String>,
    smtp: Smtp,
    client_timeout: Option<tokio::time::Duration>,
) -> crate::error::Result<()> {
    deliver_email(subject, EmailBody::Html(content), to, smtp, client_timeout).await
}

#[cfg(not(feature = "private"))]
pub async fn send_email_html(
    subject: &str,
    content: &str,
    to: Vec<String>,
    smtp: Smtp,
    client_timeout: Option<tokio::time::Duration>,
) -> crate::error::Result<()> {
    deliver_email(subject, EmailBody::Html(content), to, smtp, client_timeout).await
}

#[cfg(not(feature = "private"))]
pub async fn send_email_plain_text(
    subject: &str,
    content: &str,
    to: Vec<String>,
    smtp: Smtp,
    client_timeout: Option<tokio::time::Duration>,
) -> crate::error::Result<()> {
    deliver_email(subject, EmailBody::Text(content), to, smtp, client_timeout).await
}

#[cfg(not(feature = "private"))]
async fn send_email_if_possible_async(
    subject: &str,
    content: &str,
    to: &str,
) -> crate::error::Result<()> {
    let smtp = crate::worker::SMTP_CONFIG.load_full();
    let Some(smtp) = smtp.as_ref().as_ref() else {
        tracing::warn!("SMTP not configured, cannot send email to {to}");
        return Ok(());
    };
    send_email_html(subject, content, vec![to.to_string()], smtp.clone(), None).await
}

#[cfg(not(feature = "private"))]
pub fn send_email_if_possible(subject: &str, content: &str, to: &str) {
    let subject = subject.to_string();
    let content = content.to_string();
    let to = to.to_string();
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => {
            handle.spawn(async move {
                if let Err(e) = send_email_if_possible_async(&subject, &content, &to).await {
                    tracing::error!("Failed to send email to {to}: {e}");
                }
            });
        }
        Err(_) => {
            tracing::warn!(
                "send_email_if_possible called outside a tokio runtime; email to {to} not sent"
            );
        }
    }
}
