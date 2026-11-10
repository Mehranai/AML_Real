use crate::{Store, now};
use anyhow::{Context, Result, ensure};
use reqwest::{Client, Url, header};
use serde_json::{Value, json};
use std::time::Duration;

const MAX_ATTEMPTS: i64 = 6;
const MAX_DELAY_MS: i64 = 900_000;
const LEASE_MS: i64 = 45_000;

pub(super) struct Webhook {
    client: Client,
    url: Url,
    authorization: Option<header::HeaderValue>,
}

impl Webhook {
    #[cfg(test)]
    pub(super) fn fixture(url: Url) -> Result<Self> {
        Ok(Self {
            client: client()?,
            url,
            authorization: Some(header::HeaderValue::from_static("Bearer fixture-token")),
        })
    }

    pub(super) fn from_env() -> Result<Option<Self>> {
        let url = std::env::var("AML_KYT_WEBHOOK_URL").unwrap_or_default();
        if url.is_empty() {
            return Ok(None);
        }
        let url = Url::parse(&url).context("invalid AML_KYT_WEBHOOK_URL")?;
        ensure!(
            url.scheme() == "https"
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none(),
            "AML_KYT_WEBHOOK_URL requires HTTPS with no credentials, query or fragment"
        );
        let token = std::env::var("AML_KYT_WEBHOOK_TOKEN").unwrap_or_default();
        let authorization = if token.is_empty() {
            None
        } else {
            let mut value = header::HeaderValue::from_str(&format!("Bearer {token}"))
                .context("invalid AML_KYT_WEBHOOK_TOKEN")?;
            value.set_sensitive(true);
            Some(value)
        };
        Ok(Some(Self {
            client: client()?,
            url,
            authorization,
        }))
    }

    async fn send(&self, id: &str, payload: &Value) -> Outcome {
        let mut request = self
            .client
            .post(self.url.clone())
            .header("Idempotency-Key", id)
            .json(payload);
        if let Some(value) = &self.authorization {
            request = request.header(header::AUTHORIZATION, value.clone());
        }
        let Ok(response) = request.send().await else {
            return Outcome {
                status: None,
                error: Some("request_failed"),
                retry: true,
                retry_after_ms: 0,
            };
        };
        let status = response.status();
        let retry = matches!(status.as_u16(), 408 | 425 | 429) || status.is_server_error();
        let retry_after_ms = response
            .headers()
            .get(header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .map(|seconds| seconds.min(86_400) as i64 * 1_000)
            .unwrap_or(0);
        Outcome {
            status: Some(status.as_u16()),
            error: (!status.is_success()).then_some("http_error"),
            retry,
            retry_after_ms,
        }
    }
}

fn client() -> Result<Client> {
    Ok(Client::builder()
        .connect_timeout(Duration::from_secs(4))
        .timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}

struct Outcome {
    status: Option<u16>,
    error: Option<&'static str>,
    retry: bool,
    retry_after_ms: i64,
}

pub(super) async fn run(store: &Store, webhook: Option<Webhook>) {
    let mut interval =
        tokio::time::interval(Duration::from_secs(if webhook.is_some() { 5 } else { 60 }));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        interval.tick().await;
        let result = match &webhook {
            Some(webhook) => tick(store, webhook).await,
            None => expire_backlog(store, now()).await,
        };
        if let Err(error) = result {
            eprintln!("KYT delivery queue failed: {error:#}");
        }
    }
}

async fn expire_backlog(store: &Store, time: i64) -> Result<()> {
    store
        .query(
            r#"
        MATCH (a:KytAlert)
        WHERE a.delivery_expires_at_ms <= $now
          AND (a.delivery_status <> 'sending' OR a.delivery_due_at_ms <= $now)
        WITH a ORDER BY a.delivery_expires_at_ms LIMIT 50
        SET a.delivery_lock=a.delivery_lock+1
        WITH a WHERE a.delivery_expires_at_ms <= $now
          AND (a.delivery_status <> 'sending' OR a.delivery_due_at_ms <= $now)
        SET a.delivery_status='failed',a.delivery_last_error='queue_expired',
            a.delivery_due_at_ms=null,a.delivery_lease=null,a.delivery_expires_at_ms=null
        RETURN count(a)"#,
            json!({"now":time}),
        )
        .await?;
    Ok(())
}

const CLAIM: &str = r#"
    MATCH (a:KytAlert)
    WHERE a.delivery_status IN ['pending','retrying','sending']
      AND a.delivery_due_at_ms <= $now AND a.delivery_expires_at_ms > $now
    WITH a ORDER BY a.delivery_due_at_ms,a.id LIMIT 1
    SET a.delivery_lock=a.delivery_lock+1
    WITH a WHERE a.delivery_status IN ['pending','retrying','sending']
      AND a.delivery_due_at_ms <= $now AND a.delivery_expires_at_ms > $now
    WITH a,a.delivery_attempts < $max AS eligible
    SET a.delivery_status=CASE WHEN eligible THEN 'sending' ELSE 'failed' END,
        a.delivery_attempts=a.delivery_attempts + CASE WHEN eligible THEN 1 ELSE 0 END,
        a.delivery_last_error=CASE WHEN eligible THEN a.delivery_last_error ELSE 'attempt_limit' END,
        a.delivery_last_attempt_at_ms=CASE WHEN eligible THEN $now ELSE a.delivery_last_attempt_at_ms END,
        a.delivery_due_at_ms=CASE WHEN eligible THEN $lease_end ELSE null END,
        a.delivery_lease=CASE WHEN eligible THEN $lease ELSE null END,
        a.delivery_expires_at_ms=CASE WHEN eligible THEN a.delivery_expires_at_ms ELSE null END
    RETURN CASE WHEN eligible THEN a{.id,.watch_id,.network_id,.kind,.value,.system_id,.customer_id,
        .rule,.threshold,.severity,.created_at_ms,.evidence_json,.delivery_attempts,.delivery_expires_at_ms}
        ELSE null END
"#;

pub(super) async fn tick(store: &Store, webhook: &Webhook) -> Result<()> {
    let time = now();
    expire_backlog(store, time).await?;
    let lease = nanoid::nanoid!(32);
    let rows = store
        .query(
            CLAIM,
            json!({"now":time,"lease":lease,"lease_end":time+LEASE_MS,
        "max":MAX_ATTEMPTS}),
        )
        .await?;
    let Some(job) = rows
        .first()
        .map(|row| &row[0])
        .filter(|job| job.is_object())
    else {
        return Ok(());
    };
    let id = job["id"].as_str().context("invalid KYT delivery ID")?;
    let attempt = job["delivery_attempts"]
        .as_i64()
        .context("invalid KYT attempt count")?;
    let expiry = job["delivery_expires_at_ms"]
        .as_i64()
        .context("invalid KYT delivery deadline")?;
    let mut alert = job.as_object().context("invalid KYT delivery")?.clone();
    alert.remove("delivery_attempts");
    alert.remove("delivery_expires_at_ms");
    let stored = alert
        .remove("evidence_json")
        .context("missing webhook evidence")?;
    alert.insert(
        "evidence".into(),
        serde_json::from_str(stored.as_str().context("invalid webhook evidence")?)?,
    );
    let outcome = webhook
        .send(
            id,
            &json!({"event":"kyt.alert.created","version":1,"alert":alert}),
        )
        .await;
    let checked = now();
    let (status, next, error) = if outcome.error.is_none() {
        ("delivered", None, None)
    } else if outcome.retry && attempt < MAX_ATTEMPTS && checked < expiry {
        let delay = (30_000 * (1_i64 << (attempt - 1)))
            .min(MAX_DELAY_MS)
            .max(outcome.retry_after_ms);
        (
            "retrying",
            Some((checked + delay).min(expiry)),
            outcome.error,
        )
    } else {
        let error = if checked >= expiry {
            Some("queue_expired")
        } else if outcome.retry {
            Some("attempt_limit")
        } else {
            outcome.error
        };
        ("failed", None, error)
    };
    store.query(r#"
        MATCH (a:KytAlert {id:$id})
        SET a.delivery_lock=a.delivery_lock+1
        WITH a WHERE a.delivery_status='sending' AND a.delivery_lease=$lease
        SET a.delivery_status=$status,a.delivery_due_at_ms=$next,a.delivery_lease=null,
            a.delivery_expires_at_ms=CASE WHEN $next IS NULL THEN null ELSE a.delivery_expires_at_ms END,
            a.delivery_last_error=$error,a.delivery_last_http_status=$http,
            a.delivered_at_ms=CASE WHEN $status='delivered' THEN $checked ELSE null END
        RETURN a.id"#, json!({"id":id,"lease":lease,"status":status,"next":next,
            "error":error,"http":outcome.status,"checked":checked})).await?;
    Ok(())
}
