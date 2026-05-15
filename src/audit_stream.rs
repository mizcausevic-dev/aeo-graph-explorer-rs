//! Optional audit-stream-py producer.
//!
//! When the `audit-stream` Cargo feature is enabled (the default for the
//! binary service) **and** the `AUDIT_STREAM_URL` env var is set, the
//! service fires one governance event per atomic state change:
//!
//! - `graph_ingested`       on a successful `POST /ingest`
//! - `graph_ingest_failed`  on a malformed `POST /ingest` (4xx response)
//!
//! Same opt-in pattern as the four Python producers and `incident-correlation`.
//! Identical env-var contract:
//!
//! - `AUDIT_STREAM_URL`        — base URL, e.g. `http://audit.local:8093`
//! - `AUDIT_STREAM_TIMEOUT_S`  — per-call timeout, default 2.5s
//!
//! Best-effort. Failures are logged to stderr and swallowed — an
//! audit-stream outage must never block graph ingestion.

use std::env;
use std::time::Duration;

use serde_json::json;

/// Default per-call timeout when `AUDIT_STREAM_TIMEOUT_S` is unset.
pub const DEFAULT_TIMEOUT_S: f64 = 2.5;

/// True when `AUDIT_STREAM_URL` is set to a non-empty value.
#[must_use]
pub fn is_enabled() -> bool {
    base_url().is_some()
}

/// Stripped audit-stream base URL, or `None` when disabled.
#[must_use]
pub fn base_url() -> Option<String> {
    let raw = env::var("AUDIT_STREAM_URL").ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.trim_end_matches('/').to_string())
}

/// Configured per-call timeout. Defaults to 2.5 seconds.
#[must_use]
pub fn timeout() -> Duration {
    let secs = env::var("AUDIT_STREAM_TIMEOUT_S")
        .ok()
        .and_then(|raw| raw.trim().parse::<f64>().ok())
        .map_or(DEFAULT_TIMEOUT_S, |v| v.max(0.1));
    Duration::from_secs_f64(secs)
}

/// Fire one event. Silent no-op when `AUDIT_STREAM_URL` is unset.
///
/// Failures (connection refused, HTTP 5xx, timeout, malformed URL) are
/// logged to stderr and swallowed — this never returns an error.
pub async fn emit(client: &reqwest::Client, kind: &str, payload: serde_json::Value) {
    let Some(url) = base_url() else {
        return;
    };
    let body = json!({
        "kind": kind,
        "source": "aeo-graph-explorer",
        "payload": payload,
    });
    let endpoint = format!("{url}/events");
    let result = client
        .post(&endpoint)
        .json(&body)
        .timeout(timeout())
        .send()
        .await;
    match result {
        Ok(resp) if resp.status().is_success() => {}
        Ok(resp) => {
            eprintln!(
                "audit-stream emit failed (kind={kind}): HTTP {}",
                resp.status()
            );
        }
        Err(err) => {
            eprintln!("audit-stream emit failed (kind={kind}): {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // AUDIT_STREAM_URL / AUDIT_STREAM_TIMEOUT_S are process-global, so
    // tests that mutate them must run serially.
    static ENV_GUARD: Mutex<()> = Mutex::new(());

    fn reset_env() {
        env::remove_var("AUDIT_STREAM_URL");
        env::remove_var("AUDIT_STREAM_TIMEOUT_S");
    }

    #[test]
    fn disabled_when_unset() {
        let _l = ENV_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset_env();
        assert!(!is_enabled());
        assert!(base_url().is_none());
    }

    #[test]
    fn disabled_when_blank() {
        let _l = ENV_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset_env();
        env::set_var("AUDIT_STREAM_URL", "   ");
        assert!(!is_enabled());
        env::remove_var("AUDIT_STREAM_URL");
    }

    #[test]
    fn enabled_with_value() {
        let _l = ENV_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset_env();
        env::set_var("AUDIT_STREAM_URL", "http://audit.local:8093");
        assert!(is_enabled());
        assert_eq!(base_url().unwrap(), "http://audit.local:8093");
        env::remove_var("AUDIT_STREAM_URL");
    }

    #[test]
    fn trailing_slash_stripped() {
        let _l = ENV_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset_env();
        env::set_var("AUDIT_STREAM_URL", "http://audit.local:8093/");
        assert_eq!(base_url().unwrap(), "http://audit.local:8093");
        env::remove_var("AUDIT_STREAM_URL");
    }

    #[test]
    fn timeout_default() {
        let _l = ENV_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset_env();
        assert_eq!(timeout(), Duration::from_secs_f64(DEFAULT_TIMEOUT_S));
    }

    #[test]
    fn timeout_override() {
        let _l = ENV_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset_env();
        env::set_var("AUDIT_STREAM_TIMEOUT_S", "5.0");
        assert_eq!(timeout(), Duration::from_secs_f64(5.0));
        env::remove_var("AUDIT_STREAM_TIMEOUT_S");
    }

    #[test]
    fn timeout_bad_value_falls_back() {
        let _l = ENV_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset_env();
        env::set_var("AUDIT_STREAM_TIMEOUT_S", "not-a-number");
        assert_eq!(timeout(), Duration::from_secs_f64(DEFAULT_TIMEOUT_S));
        env::remove_var("AUDIT_STREAM_TIMEOUT_S");
    }
}
