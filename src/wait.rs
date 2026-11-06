//! Polling note/paragraph status until execution completes.

use std::time::{Duration, Instant};

use anyhow::Result;
use serde_json::{Value, json};

use crate::cli::WaitArgs;
use crate::client::{ApiError, Client};
use crate::output::Output;

pub const EXIT_FAILED: i32 = 3;
pub const EXIT_TIMEOUT: i32 = 4;

/// Paragraph states that mean "still going".
fn is_active(status: &str) -> bool {
    matches!(status, "PENDING" | "RUNNING")
}

fn is_failure(status: &str) -> bool {
    matches!(status, "ERROR" | "ABORTED")
}

/// Normalised view of `GET /notebook/job/{id}`.
///
/// Zeppelin 0.11 returns `{"id", "isRunning", "paragraphs": [...]}`; older servers
/// (and the published docs) return a bare array. Both are accepted.
pub struct NoteStatus {
    pub is_running: bool,
    pub paragraphs: Vec<Value>,
}

impl NoteStatus {
    pub fn from_body(body: &Value) -> NoteStatus {
        match body {
            Value::Array(items) => NoteStatus {
                is_running: false,
                paragraphs: items.clone(),
            },
            _ => NoteStatus {
                is_running: body
                    .get("isRunning")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                paragraphs: body
                    .get("paragraphs")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default(),
            },
        }
    }

    fn statuses(&self) -> impl Iterator<Item = &str> {
        self.paragraphs
            .iter()
            .map(|p| p.get("status").and_then(Value::as_str).unwrap_or("UNKNOWN"))
    }

    pub fn done(&self) -> bool {
        !self.is_running && !self.statuses().any(is_active)
    }

    pub fn failed(&self) -> bool {
        self.statuses().any(is_failure)
    }

    fn summary(&self) -> String {
        let mut counts: Vec<(String, usize)> = Vec::new();
        for s in self.statuses() {
            match counts.iter_mut().find(|(k, _)| k == s) {
                Some((_, n)) => *n += 1,
                None => counts.push((s.to_string(), 1)),
            }
        }
        let parts: Vec<String> = counts.iter().map(|(k, n)| format!("{n} {k}")).collect();
        format!(
            "{} — {}",
            if self.is_running { "running" } else { "idle" },
            if parts.is_empty() {
                "no paragraphs".to_string()
            } else {
                parts.join(", ")
            }
        )
    }
}

fn is_server_error(e: &anyhow::Error) -> bool {
    e.downcast_ref::<ApiError>()
        .is_some_and(|a| a.http_status >= 500)
}

/// Text of a paragraph's results, used as the error message in the fallback.
fn result_text(p: &Value) -> Option<String> {
    let msgs = p.pointer("/results/msg")?.as_array()?;
    let text: Vec<&str> = msgs
        .iter()
        .filter_map(|m| m.get("data").and_then(Value::as_str))
        .collect();
    (!text.is_empty()).then(|| text.join("").trim().to_string())
}

/// Build a job-status-shaped object from a full paragraph.
fn status_from_paragraph(p: &Value) -> Value {
    let status = p.get("status").and_then(Value::as_str).unwrap_or("UNKNOWN");
    let mut s = json!({
        "id": p.get("id").cloned().unwrap_or(Value::Null),
        "status": status,
        "started": p.get("dateStarted").cloned().unwrap_or(Value::Null),
        "finished": p.get("dateFinished").cloned().unwrap_or(Value::Null),
    });
    if is_failure(status)
        && let Some(t) = result_text(p)
    {
        s["errorMessage"] = Value::String(t);
    }
    s
}

/// Fetches status from the job endpoints, falling back to the full note/paragraph
/// when they fail. Some Zeppelin builds (including the apache/zeppelin:0.11.0 Docker
/// image) answer `GET /notebook/job/...` with HTTP 500 once any paragraph is in ERROR.
#[derive(Default)]
pub struct StatusSource {
    use_fallback: bool,
}

impl StatusSource {
    /// Returns the response envelope of `GET /notebook/job/{note}` (or an equivalent).
    pub fn note(&mut self, client: &Client, note_id: &str) -> Result<Value> {
        if !self.use_fallback {
            match client.get(&["notebook", "job", note_id]) {
                Ok(env) => return Ok(env),
                Err(e) if is_server_error(&e) => self.use_fallback = true,
                Err(e) => return Err(e),
            }
        }
        let env = client.get(&["notebook", note_id])?;
        let note = env.get("body").cloned().unwrap_or(Value::Null);
        let paragraphs: Vec<Value> = note
            .get("paragraphs")
            .and_then(Value::as_array)
            .map(|ps| ps.iter().map(status_from_paragraph).collect())
            .unwrap_or_default();
        Ok(json!({"status": "OK", "body": {
            "id": note_id,
            "isRunning": note.pointer("/info/isRunning").and_then(Value::as_bool).unwrap_or(false),
            "paragraphs": paragraphs,
        }}))
    }

    /// Returns the response envelope of `GET /notebook/job/{note}/{paragraph}` (or an equivalent).
    pub fn paragraph(&mut self, client: &Client, note_id: &str, para_id: &str) -> Result<Value> {
        if !self.use_fallback {
            match client.get(&["notebook", "job", note_id, para_id]) {
                Ok(env) => return Ok(env),
                Err(e) if is_server_error(&e) => self.use_fallback = true,
                Err(e) => return Err(e),
            }
        }
        let env = client.get(&["notebook", note_id, "paragraph", para_id])?;
        let p = env.get("body").cloned().unwrap_or(Value::Null);
        Ok(json!({"status": "OK", "body": status_from_paragraph(&p)}))
    }
}

struct Poller {
    args: WaitArgs,
    start: Instant,
    last_line: String,
}

impl Poller {
    fn new(args: &WaitArgs) -> Self {
        Poller {
            args: args.clone(),
            start: Instant::now(),
            last_line: String::new(),
        }
    }

    fn report(&mut self, what: &str, line: String) {
        if !self.args.quiet && line != self.last_line {
            eprintln!(
                "[{:>5.0}s] {what}: {line}",
                self.start.elapsed().as_secs_f64()
            );
            self.last_line = line;
        }
    }

    /// Sleep until the next poll. Returns false if the timeout has passed.
    fn tick(&self) -> bool {
        if self.args.timeout > 0.0 && self.start.elapsed().as_secs_f64() >= self.args.timeout {
            return false;
        }
        std::thread::sleep(Duration::from_secs_f64(self.args.interval.max(0.1)));
        true
    }
}

/// Wait for a whole note. Prints the final status body on stdout and returns the exit code.
pub fn note(client: &Client, out: Output, note_id: &str, args: &WaitArgs) -> Result<i32> {
    let mut poller = Poller::new(args);
    let mut source = StatusSource::default();
    loop {
        let envelope = source.note(client, note_id)?;
        let body = envelope.get("body").cloned().unwrap_or(Value::Null);
        let status = NoteStatus::from_body(&body);
        poller.report(&format!("note {note_id}"), status.summary());

        if status.done() {
            out.response(&envelope);
            if status.failed() {
                for p in &status.paragraphs {
                    let s = p.get("status").and_then(Value::as_str).unwrap_or("");
                    if is_failure(s) {
                        let id = p.get("id").and_then(Value::as_str).unwrap_or("?");
                        let msg = p.get("errorMessage").and_then(Value::as_str).unwrap_or("");
                        eprintln!(
                            "paragraph {id} {s}{}{msg}",
                            if msg.is_empty() { "" } else { ": " }
                        );
                    }
                }
                return Ok(EXIT_FAILED);
            }
            return Ok(0);
        }
        if !poller.tick() {
            out.response(&envelope);
            eprintln!(
                "timed out after {}s waiting for note {note_id}",
                args.timeout
            );
            return Ok(EXIT_TIMEOUT);
        }
    }
}

/// Wait for one paragraph.
pub fn paragraph(
    client: &Client,
    out: Output,
    note_id: &str,
    para_id: &str,
    args: &WaitArgs,
) -> Result<i32> {
    let mut poller = Poller::new(args);
    let mut source = StatusSource::default();
    loop {
        let envelope = source.paragraph(client, note_id, para_id)?;
        let body = envelope.get("body").cloned().unwrap_or(json!({}));
        let status = body
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("UNKNOWN")
            .to_string();
        let progress = body.get("progress").and_then(Value::as_str).unwrap_or("");
        let line = if is_active(&status) && !progress.is_empty() {
            format!("{status} ({progress}%)")
        } else {
            status.clone()
        };
        poller.report(&format!("paragraph {para_id}"), line);

        if !is_active(&status) {
            out.response(&envelope);
            if is_failure(&status) {
                if let Some(msg) = body.get("errorMessage").and_then(Value::as_str) {
                    eprintln!("paragraph {para_id} {status}: {msg}");
                }
                return Ok(EXIT_FAILED);
            }
            return Ok(0);
        }
        if !poller.tick() {
            out.response(&envelope);
            eprintln!(
                "timed out after {}s waiting for paragraph {para_id}",
                args.timeout
            );
            return Ok(EXIT_TIMEOUT);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_shape() {
        let s = NoteStatus::from_body(&json!({
            "id": "N", "isRunning": true,
            "paragraphs": [{"id": "a", "status": "FINISHED"}, {"id": "b", "status": "READY"}]
        }));
        assert!(!s.done(), "note still flagged running");
        let s = NoteStatus::from_body(&json!({
            "id": "N", "isRunning": false,
            "paragraphs": [{"id": "a", "status": "FINISHED"}, {"id": "b", "status": "ERROR"}]
        }));
        assert!(s.done() && s.failed());
    }

    #[test]
    fn array_shape() {
        let s = NoteStatus::from_body(&json!([{"id": "a", "status": "RUNNING"}]));
        assert!(!s.done());
        let s = NoteStatus::from_body(&json!([{"id": "a", "status": "FINISHED"}]));
        assert!(s.done() && !s.failed());
    }
}
