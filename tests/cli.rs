//! End-to-end tests: run the binary against a mock Zeppelin server.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use assert_cmd::Command;
use mockito::{Matcher, Server};
use predicates::prelude::*;
use serde_json::json;
use tempfile::TempDir;

struct Env {
    dir: TempDir,
}

impl Env {
    fn new(config: &str) -> Env {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join(".zeppelin.toml"), config).unwrap();
        Env { dir }
    }

    fn cmd(&self) -> Command {
        let mut c = Command::cargo_bin("zeppelin").unwrap();
        // HOME controls where ~/.zeppelin.toml is found; keep the sandbox proxy out of local calls.
        c.env("HOME", self.dir.path())
            .env("NO_PROXY", "127.0.0.1,localhost")
            .env_remove("ZEPPELIN_PROFILE")
            .env_remove("ZEPPELIN_URL")
            .env_remove("ZEPPELIN_USERNAME")
            .env_remove("ZEPPELIN_PASSWORD")
            .env_remove("ZEPPELIN_CONFIG");
        c
    }
}

fn config_for(url: &str) -> String {
    format!(
        r#"
default_profile = "local"
username = "admin"
password = "password1"

[profiles.local]
base_url = "{url}"
"#
    )
}

fn login_mock(server: &mut Server, user: &str, pass: &str) -> mockito::Mock {
    server
        .mock("POST", "/api/login")
        .match_body(Matcher::AllOf(vec![
            Matcher::UrlEncoded("userName".into(), user.into()),
            Matcher::UrlEncoded("password".into(), pass.into()),
        ]))
        .with_header("set-cookie", "JSESSIONID=abc123; Path=/")
        .with_body(r#"{"status":"OK"}"#)
        .create()
}

#[test]
fn logs_in_with_default_credentials_and_sends_cookie() {
    let mut server = Server::new();
    let login = login_mock(&mut server, "admin", "password1");
    let list = server
        .mock("GET", "/api/notebook")
        .match_header("cookie", Matcher::Regex("JSESSIONID=abc123".into()))
        .with_body(r#"{"status":"OK","message":"","body":[{"id":"A","path":"/Team/Report"},{"id":"B","path":"/Scratch"}]}"#)
        .create();

    let env = Env::new(&config_for(&server.url()));
    env.cmd()
        .args(["note", "list", "--filter", "team", "--compact"])
        .assert()
        .success()
        .stdout(r#"[{"id":"A","path":"/Team/Report"}]"#.to_owned() + "\n");
    login.assert();
    list.assert();
}

#[test]
fn profile_credentials_override_top_level() {
    let mut server = Server::new();
    let login = login_mock(&mut server, "svc", "s3cret");
    server
        .mock("GET", "/api/version")
        .with_body(r#"{"status":"OK","body":{"version":"0.11.0"}}"#)
        .create();

    let config = format!(
        r#"
username = "admin"
password = "password1"
[profiles.local]
base_url = "http://127.0.0.1:1"
[profiles.prod]
base_url = "{}"
username = "svc"
password = "s3cret"
"#,
        server.url()
    );
    let env = Env::new(&config);
    env.cmd()
        .args(["--profile", "prod", "ping", "--compact"])
        .assert()
        .success()
        .stdout("{\"version\":\"0.11.0\"}\n");
    login.assert();
}

#[test]
fn create_note_from_stdin_and_flags() {
    let mut server = Server::new();
    login_mock(&mut server, "admin", "password1");
    let create = server
        .mock("POST", "/api/notebook")
        .match_body(Matcher::Json(json!({
            "name": "/tmp/demo",
            "paragraphs": [
                {"title": "one", "text": "%sh echo 1"},
                {"text": "%md hello"}
            ],
            "addingEmptyParagraph": false
        })))
        .with_body(r#"{"status":"OK","message":"","body":"2NEWNOTE"}"#)
        .create();

    let env = Env::new(&config_for(&server.url()));
    env.cmd()
        .args([
            "note",
            "create",
            "--name",
            "/tmp/demo",
            "--from",
            "-",
            "--paragraph",
            "%md hello",
        ])
        .write_stdin(r#"[{"title":"one","text":"%sh echo 1"}]"#)
        .assert()
        .success()
        .stdout("2NEWNOTE\n");
    create.assert();
}

#[test]
fn run_and_wait_polls_until_done() {
    let mut server = Server::new();
    login_mock(&mut server, "admin", "password1");
    server
        .mock("POST", "/api/notebook/job/N1")
        .with_body(r#"{"status":"OK"}"#)
        .create();

    let polls = Arc::new(AtomicUsize::new(0));
    let counter = polls.clone();
    server
        .mock("GET", "/api/notebook/job/N1")
        .with_body_from_request(move |_| {
            let n = counter.fetch_add(1, Ordering::SeqCst);
            let body = if n < 2 {
                json!({"status":"OK","body":{"id":"N1","isRunning":true,"paragraphs":[
                    {"id":"p1","status":"FINISHED"},{"id":"p2","status":"RUNNING","progress":"50"}]}})
            } else {
                json!({"status":"OK","body":{"id":"N1","isRunning":false,"paragraphs":[
                    {"id":"p1","status":"FINISHED"},{"id":"p2","status":"FINISHED"}]}})
            };
            body.to_string().into_bytes()
        })
        .expect_at_least(3)
        .create();

    let env = Env::new(&config_for(&server.url()));
    env.cmd()
        .args(["note", "run", "N1", "--wait", "--interval", "0.1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"isRunning\": false"))
        .stderr(predicate::str::contains("1 FINISHED, 1 RUNNING"));
    assert!(polls.load(Ordering::SeqCst) >= 3);
}

#[test]
fn wait_exits_3_on_paragraph_error() {
    let mut server = Server::new();
    login_mock(&mut server, "admin", "password1");
    server
        .mock("GET", "/api/notebook/job/N1")
        .with_body(
            json!({"status":"OK","body":{"id":"N1","isRunning":false,"paragraphs":[
                {"id":"p1","status":"ERROR","errorMessage":"boom"}]}})
            .to_string(),
        )
        .create();
    let env = Env::new(&config_for(&server.url()));
    env.cmd()
        .args(["note", "wait", "N1", "--quiet"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains("p1 ERROR: boom"));
}

#[test]
fn wait_times_out_with_exit_4() {
    let mut server = Server::new();
    login_mock(&mut server, "admin", "password1");
    server
        .mock("GET", "/api/notebook/job/N1/p1")
        .with_body(r#"{"status":"OK","body":{"id":"p1","status":"RUNNING","progress":"10"}}"#)
        .create();
    let env = Env::new(&config_for(&server.url()));
    env.cmd()
        .args([
            "paragraph",
            "wait",
            "N1",
            "p1",
            "--interval",
            "0.1",
            "--timeout",
            "0.3",
        ])
        .assert()
        .code(4)
        .stderr(predicate::str::contains("timed out"));
}

#[test]
fn sync_run_failure_exits_3_and_prints_result() {
    let mut server = Server::new();
    login_mock(&mut server, "admin", "password1");
    server
        .mock("POST", "/api/notebook/run/N1/p1")
        .match_body(Matcher::Json(json!({"params": {"x": "1"}})))
        .with_status(500)
        .with_body(r#"{"status":"INTERNAL_SERVER_ERROR","body":{"code":"ERROR","msg":[{"type":"TEXT","data":"bad"}]}}"#)
        .create();
    let env = Env::new(&config_for(&server.url()));
    env.cmd()
        .args([
            "paragraph",
            "run",
            "N1",
            "p1",
            "--sync",
            "--params",
            r#"{"x":"1"}"#,
        ])
        .assert()
        .code(3)
        .stdout(predicate::str::contains("\"code\": \"ERROR\""));
}

#[test]
fn export_then_import_with_new_name() {
    let mut server = Server::new();
    login_mock(&mut server, "admin", "password1");
    let note = json!({"name": "orig", "paragraphs": [{"text": "%md hi"}]});
    server
        .mock("GET", "/api/notebook/export/N1")
        .with_body(json!({"status":"OK","message":"","body": note.to_string()}).to_string())
        .create();
    let import = server
        .mock("POST", "/api/notebook/import")
        .match_query(Matcher::UrlEncoded(
            "notePath".into(),
            "/copies/copy".into(),
        ))
        .match_body(Matcher::Json(
            json!({"name": "copy", "paragraphs": [{"text": "%md hi"}]}),
        ))
        .with_body(r#"{"status":"OK","message":"","body":"N2"}"#)
        .create();

    let env = Env::new(&config_for(&server.url()));
    let file = env.dir.path().join("note.json");
    env.cmd()
        .args(["note", "export", "N1", "-o", file.to_str().unwrap()])
        .assert()
        .success();
    let exported: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(exported, note);

    env.cmd()
        .args([
            "note",
            "import",
            file.to_str().unwrap(),
            "--name",
            "copy",
            "--path",
            "copies/copy",
        ])
        .assert()
        .success()
        .stdout("N2\n");
    import.assert();
}

#[test]
fn api_errors_are_reported() {
    let mut server = Server::new();
    login_mock(&mut server, "admin", "password1");
    server
        .mock("GET", "/api/notebook/NOPE")
        .with_status(404)
        .with_body(r#"{"status":"NOT_FOUND","message":"Note NOPE not found"}"#)
        .create();
    let env = Env::new(&config_for(&server.url()));
    env.cmd()
        .args(["note", "get", "NOPE"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "HTTP 404 NOT_FOUND: Note NOPE not found",
        ));
}

#[test]
fn failed_login_is_reported() {
    let mut server = Server::new();
    server.mock("POST", "/api/login").with_status(403).create();
    let env = Env::new(&config_for(&server.url()));
    env.cmd()
        .args(["note", "list"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "login as 'admin' failed (HTTP 403)",
        ));
}

#[test]
fn config_show_hides_password() {
    let env = Env::new(&config_for("http://zep:8080"));
    env.cmd()
        .args(["config", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"api_root\": \"http://zep:8080/api\"",
        ))
        .stdout(predicate::str::contains("password1").not());
}

#[test]
fn status_falls_back_to_note_when_job_endpoint_fails() {
    let mut server = Server::new();
    login_mock(&mut server, "admin", "password1");
    server
        .mock("GET", "/api/notebook/job/N1")
        .with_status(500)
        .create();
    server
        .mock("GET", "/api/notebook/N1")
        .with_body(
            json!({"status":"OK","body":{"id":"N1","info":{"isRunning":false},"paragraphs":[
                {"id":"p1","status":"FINISHED"},
                {"id":"p2","status":"ERROR","results":{"code":"ERROR","msg":[{"type":"TEXT","data":"ExitValue: 3"}]}}
            ]}})
            .to_string(),
        )
        .create();
    let env = Env::new(&config_for(&server.url()));
    env.cmd()
        .args(["note", "wait", "N1", "--quiet", "--compact"])
        .assert()
        .code(3)
        .stdout(predicate::str::contains(r#""errorMessage":"ExitValue: 3""#))
        .stderr(predicate::str::contains("p2 ERROR: ExitValue: 3"));
}

#[test]
fn note_id_resolves_path_from_list() {
    let mut server = Server::new();
    login_mock(&mut server, "admin", "password1");
    server
        .mock("GET", "/api/notebook")
        .with_body(r#"{"status":"OK","body":[{"id":"A","path":"/x/report"},{"id":"B","path":"/x/report 2"}]}"#)
        .create();
    let env = Env::new(&config_for(&server.url()));
    env.cmd()
        .args(["note", "id", "x/report"])
        .assert()
        .success()
        .stdout("A\n");
    env.cmd().args(["note", "id", "/missing"]).assert().code(1);
}

#[test]
fn paragraph_config_reads_json_from_stdin() {
    let mut server = Server::new();
    login_mock(&mut server, "admin", "password1");
    let m = server
        .mock("PUT", "/api/notebook/N1/paragraph/p1/config")
        .match_body(Matcher::Json(json!({"colWidth": 6})))
        .with_body(r#"{"status":"OK","body":{"id":"p1","config":{"colWidth":6.0}}}"#)
        .create();
    let env = Env::new(&config_for(&server.url()));
    env.cmd()
        .args(["paragraph", "config", "N1", "p1", "-"])
        .write_stdin(r#"{"colWidth": 6}"#)
        .assert()
        .success();
    m.assert();
}

#[test]
fn html_error_pages_are_summarised() {
    let mut server = Server::new();
    login_mock(&mut server, "admin", "password1");
    server
        .mock("GET", "/api/notebook/N1/revision")
        .with_status(404)
        .with_body("<!DOCTYPE html><html>nope</html>")
        .create();
    let env = Env::new(&config_for(&server.url()));
    env.cmd()
        .args(["revision", "list", "N1"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("non-JSON (HTML) response"))
        .stderr(predicate::str::contains("DOCTYPE").not());
}
