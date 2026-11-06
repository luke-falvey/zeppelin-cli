mod cli;
mod client;
mod config;
mod input;
mod output;
mod wait;

use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;
use reqwest::Method;
use serde_json::{Map, Value, json};

use cli::*;
use client::{ApiError, Client, RequestBody};
use output::Output;

const EXAMPLE_CONFIG: &str = r#"# ~/.zeppelin.toml

# Profile used when --profile / ZEPPELIN_PROFILE are not given.
default_profile = "local"

# Default credentials, used by any profile that doesn't set its own.
username = "admin"
password = "password1"

[profiles.local]
base_url = "http://localhost:8080"

[profiles.staging]
base_url = "https://zeppelin.staging.example.com"

[profiles.prod]
base_url = "https://zeppelin.example.com"
username = "svc-zeppelin"
password = "change-me"
"#;

fn main() {
    let cli = Cli::parse();
    let code = match run(cli) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            1
        }
    };
    std::process::exit(code);
}

fn run(cli: Cli) -> Result<i32> {
    let out = Output {
        raw: cli.raw,
        compact: cli.compact,
    };
    let config_path = match &cli.config {
        Some(p) => p.clone(),
        None => config::default_path()?,
    };
    let file = config::load(&config_path)?;
    let overrides = config::Overrides {
        profile: cli.profile.clone(),
        url: cli.url.clone(),
        username: cli.username.clone(),
        password: cli.password.clone(),
    };

    // Commands that don't talk to the server.
    if let Command::Config(cmd) = &cli.command {
        match cmd {
            ConfigCmd::Path => println!("{}", config_path.display()),
            ConfigCmd::Example => print!("{EXAMPLE_CONFIG}"),
            ConfigCmd::Profiles => {
                let default = file.profile_name(None).ok().flatten();
                let list: Vec<Value> = file
                    .profiles
                    .iter()
                    .map(|(name, p)| {
                        json!({
                            "name": name,
                            "base_url": p.base_url,
                            "username": p.username.clone().or_else(|| file.username.clone()),
                            "default": Some(name) == default.as_ref(),
                        })
                    })
                    .collect();
                out.json(&Value::Array(list));
            }
            ConfigCmd::Show => {
                let r = file.resolve(&overrides)?;
                out.json(&json!({
                    "config_file": config_path.display().to_string(),
                    "profile": r.profile,
                    "base_url": r.base_url,
                    "api_root": config::api_root(&r.base_url),
                    "username": r.username,
                    "password_set": r.password.is_some(),
                }));
            }
        }
        return Ok(0);
    }

    let resolved = file.resolve(&overrides)?;
    let client = Client::connect(&resolved, Duration::from_secs(cli.http_timeout))?;
    dispatch(&client, out, cli.command)
}

fn get(client: &Client, out: Output, segs: &[&str]) -> Result<i32> {
    out.response(&client.get(segs)?);
    Ok(0)
}

fn send(
    client: &Client,
    out: Output,
    method: Method,
    segs: &[&str],
    body: Option<Value>,
) -> Result<i32> {
    out.response(&client.call(method, segs, &[], body.map(RequestBody::Json))?);
    Ok(0)
}

fn params_body(params: Option<String>) -> Result<Option<Value>> {
    params
        .map(|p| {
            let v = input::json_arg(&p)?;
            if !v.is_object() {
                bail!("--params must be a JSON object");
            }
            Ok(json!({ "params": v }))
        })
        .transpose()
}

fn text_body(args: TextArgs, require_text: bool) -> Result<Map<String, Value>> {
    let mut body = Map::new();
    let text = match (args.text, args.text_file) {
        (Some(t), _) => Some(t),
        (None, Some(f)) => Some(input::read_source(&f)?),
        (None, None) => None,
    };
    match text {
        Some(t) => {
            body.insert("text".into(), Value::String(t));
        }
        None if require_text => bail!("provide paragraph text with --text or --text-file"),
        None => {}
    }
    if let Some(t) = args.title {
        body.insert("title".into(), Value::String(t));
    }
    Ok(body)
}

fn dispatch(client: &Client, out: Output, command: Command) -> Result<i32> {
    match command {
        Command::Config(_) => unreachable!("handled before connecting"),
        Command::Ping => get(client, out, &["version"]),
        Command::Search { query } => {
            out.response(&client.call(
                Method::GET,
                &["notebook", "search"],
                &[("q", query)],
                None,
            )?);
            Ok(0)
        }
        Command::Note(cmd) => note(client, out, cmd),
        Command::Paragraph(cmd) => paragraph(client, out, cmd),
        Command::Cron(cmd) => match cmd {
            CronCmd::Get { note_id } => get(client, out, &["notebook", "cron", &note_id]),
            CronCmd::Remove { note_id } => send(
                client,
                out,
                Method::DELETE,
                &["notebook", "cron", &note_id],
                None,
            ),
            CronCmd::Set {
                note_id,
                cron,
                release_resource,
            } => send(
                client,
                out,
                Method::POST,
                &["notebook", "cron", &note_id],
                Some(json!({ "cron": cron, "releaseResource": release_resource })),
            ),
        },
        Command::Permissions(cmd) => match cmd {
            PermissionsCmd::Get { note_id } => {
                get(client, out, &["notebook", &note_id, "permissions"])
            }
            PermissionsCmd::Set {
                note_id,
                owners,
                readers,
                runners,
                writers,
            } => send(
                client,
                out,
                Method::PUT,
                &["notebook", &note_id, "permissions"],
                Some(
                    json!({ "owners": owners, "readers": readers, "runners": runners, "writers": writers }),
                ),
            ),
        },
        Command::Revision(cmd) => match cmd {
            RevisionCmd::List { note_id } => get(client, out, &["notebook", &note_id, "revision"]),
            RevisionCmd::Save { note_id, message } => send(
                client,
                out,
                Method::POST,
                &["notebook", &note_id, "revision"],
                Some(json!({ "commitMessage": message })),
            ),
            RevisionCmd::Get {
                note_id,
                revision_id,
            } => get(
                client,
                out,
                &["notebook", &note_id, "revision", &revision_id],
            ),
            RevisionCmd::Revert {
                note_id,
                revision_id,
            } => send(
                client,
                out,
                Method::PUT,
                &["notebook", &note_id, "revision", &revision_id],
                None,
            ),
        },
    }
}

fn note_path(path: &str) -> String {
    if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    }
}

/// Look a note up by path in the note list. (`POST /notebook/getByPath` is not
/// available on every 0.11 build, so the list is used instead.)
fn note_id_for_path(client: &Client, path: &str) -> Result<String> {
    let want = note_path(path);
    let env = client.get(&["notebook"])?;
    env.get("body")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|n| {
            n.get("path")
                .and_then(Value::as_str)
                .map(note_path)
                .as_deref()
                == Some(want.as_str())
        })
        .and_then(|n| n.get("id").and_then(Value::as_str))
        .map(str::to_string)
        .ok_or_else(|| anyhow!("no note found at path {want}"))
}

fn note(client: &Client, out: Output, cmd: NoteCmd) -> Result<i32> {
    match cmd {
        NoteCmd::List { filter } => {
            let mut env = client.get(&["notebook"])?;
            if let (Some(f), Some(Value::Array(notes))) = (filter, env.get_mut("body")) {
                let f = f.to_lowercase();
                notes.retain(|n| {
                    n.get("path")
                        .and_then(Value::as_str)
                        .is_some_and(|p| p.to_lowercase().contains(&f))
                });
            }
            out.response(&env);
            Ok(0)
        }
        NoteCmd::Create {
            name,
            from,
            paragraphs,
            paragraph_files,
            default_interpreter,
            add_empty_paragraph,
        } => {
            let mut body = match from {
                None => Map::new(),
                Some(src) => match input::json_source(&src)? {
                    Value::Object(m) => m,
                    Value::Array(ps) => {
                        Map::from_iter([("paragraphs".to_string(), Value::Array(ps))])
                    }
                    _ => bail!("--from must contain a JSON object or an array of paragraphs"),
                },
            };
            if let Some(n) = name {
                body.insert("name".into(), Value::String(n));
            }
            let list = body.entry("paragraphs").or_insert_with(|| json!([]));
            let list = list
                .as_array_mut()
                .ok_or_else(|| anyhow!("\"paragraphs\" must be an array"))?;
            for text in paragraphs {
                list.push(json!({ "text": text }));
            }
            for file in paragraph_files {
                list.push(json!({ "text": input::read_source(&file)? }));
            }
            if let Some(g) = default_interpreter {
                body.insert("defaultInterpreterGroup".into(), Value::String(g));
            }
            body.insert(
                "addingEmptyParagraph".into(),
                Value::Bool(add_empty_paragraph),
            );
            send(
                client,
                out,
                Method::POST,
                &["notebook"],
                Some(Value::Object(body)),
            )
        }
        NoteCmd::Get {
            note,
            by_path,
            reload,
        } => {
            let query = if reload {
                vec![("reload", "true".to_string())]
            } else {
                vec![]
            };
            let id = if by_path {
                note_id_for_path(client, &note)?
            } else {
                note
            };
            out.response(&client.call(Method::GET, &["notebook", &id], &query, None)?);
            Ok(0)
        }
        NoteCmd::Id { path } => {
            println!("{}", note_id_for_path(client, &path)?);
            Ok(0)
        }
        NoteCmd::Delete { note_id } => {
            send(client, out, Method::DELETE, &["notebook", &note_id], None)
        }
        NoteCmd::Clone {
            note_id,
            name,
            revision,
        } => {
            let mut body = Map::new();
            if let Some(n) = name {
                body.insert("name".into(), Value::String(n));
            }
            if let Some(r) = revision {
                body.insert("revisionId".into(), Value::String(r));
            }
            send(
                client,
                out,
                Method::POST,
                &["notebook", &note_id],
                Some(Value::Object(body)),
            )
        }
        NoteCmd::Rename { note_id, name } => send(
            client,
            out,
            Method::PUT,
            &["notebook", &note_id, "rename"],
            Some(json!({ "name": name })),
        ),
        NoteCmd::Export { note_id, output } => {
            let env = client.get(&["notebook", "export", &note_id])?;
            if out.raw && output.is_none() {
                out.json(&env);
                return Ok(0);
            }
            // The server returns the note JSON as a string inside "body".
            let note = match env.get("body") {
                Some(Value::String(s)) => {
                    serde_json::from_str(s).context("export body is not valid JSON")?
                }
                Some(v) => v.clone(),
                None => env.clone(),
            };
            match output {
                Some(path) => {
                    let text = serde_json::to_string_pretty(&note)?;
                    std::fs::write(&path, text + "\n")
                        .with_context(|| format!("could not write {}", path.display()))?;
                    eprintln!("exported {note_id} to {}", path.display());
                }
                None => out.json(&note),
            }
            Ok(0)
        }
        NoteCmd::Import { file, name, path } => {
            let text = input::read_source(&file)?;
            let mut note: Value = serde_json::from_str(&text).with_context(|| {
                format!(
                    "invalid note JSON in {}",
                    if file == "-" { "stdin" } else { &file }
                )
            })?;
            let body = match name {
                Some(n) => {
                    note.as_object_mut()
                        .ok_or_else(|| anyhow!("note JSON must be an object"))?
                        .insert("name".into(), Value::String(n));
                    RequestBody::Json(note)
                }
                None => RequestBody::Raw(text),
            };
            let query = path
                .map(|p| vec![("notePath", note_path(&p))])
                .unwrap_or_default();
            out.response(&client.call(
                Method::POST,
                &["notebook", "import"],
                &query,
                Some(body),
            )?);
            Ok(0)
        }
        NoteCmd::Run {
            note_id,
            params,
            blocking,
            isolated,
            wait,
            wait_args,
        } => {
            let mut query = vec![];
            if blocking {
                query.push(("blocking", "true".to_string()));
            }
            if isolated {
                query.push(("isolated", "true".to_string()));
            }
            let body = params_body(params)?.map(RequestBody::Json);
            let env = client.call(Method::POST, &["notebook", "job", &note_id], &query, body)?;
            if wait || blocking {
                // A blocking run has already finished; one status check reports the outcome.
                return wait::note(client, out, &note_id, &wait_args);
            }
            out.response(&env);
            Ok(0)
        }
        NoteCmd::Stop { note_id } => send(
            client,
            out,
            Method::DELETE,
            &["notebook", "job", &note_id],
            None,
        ),
        NoteCmd::Status { note_id } => {
            out.response(&wait::StatusSource::default().note(client, &note_id)?);
            Ok(0)
        }
        NoteCmd::Wait { note_id, wait_args } => wait::note(client, out, &note_id, &wait_args),
        NoteCmd::Output { note_id } => {
            let env = client.get(&["notebook", &note_id])?;
            let paragraphs = env
                .pointer("/body/paragraphs")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for (i, p) in paragraphs.iter().enumerate() {
                let id = p.get("id").and_then(Value::as_str).unwrap_or("?");
                let status = p.get("status").and_then(Value::as_str).unwrap_or("?");
                let title = p
                    .get("title")
                    .and_then(Value::as_str)
                    .map(|t| format!(" {t}"))
                    .unwrap_or_default();
                println!("=== [{i}]{title} ({id}) {status} ===");
                print_results(p);
            }
            Ok(0)
        }
        NoteCmd::Clear { note_id } => send(
            client,
            out,
            Method::PUT,
            &["notebook", &note_id, "clear"],
            None,
        ),
    }
}

/// Print `results.msg[].data` of a paragraph as plain text.
fn print_results(paragraph: &Value) {
    if let Some(msgs) = paragraph.pointer("/results/msg").and_then(Value::as_array) {
        for m in msgs {
            if let Some(data) = m.get("data").and_then(Value::as_str) {
                print!("{data}");
                if !data.ends_with('\n') {
                    println!();
                }
            }
        }
    }
}

fn paragraph(client: &Client, out: Output, cmd: ParagraphCmd) -> Result<i32> {
    match cmd {
        ParagraphCmd::Create {
            note_id,
            text,
            index,
            config,
        } => {
            let mut body = text_body(text, true)?;
            if let Some(i) = index {
                body.insert("index".into(), json!(i));
            }
            if let Some(c) = config {
                body.insert("config".into(), input::json_arg(&c)?);
            }
            send(
                client,
                out,
                Method::POST,
                &["notebook", &note_id, "paragraph"],
                Some(Value::Object(body)),
            )
        }
        ParagraphCmd::Get {
            note_id,
            paragraph_id,
        } => get(
            client,
            out,
            &["notebook", &note_id, "paragraph", &paragraph_id],
        ),
        ParagraphCmd::Update {
            note_id,
            paragraph_id,
            text,
        } => {
            let body = text_body(text, false)?;
            if body.is_empty() {
                bail!("nothing to update: pass --text, --text-file and/or --title");
            }
            send(
                client,
                out,
                Method::PUT,
                &["notebook", &note_id, "paragraph", &paragraph_id],
                Some(Value::Object(body)),
            )
        }
        ParagraphCmd::Config {
            note_id,
            paragraph_id,
            config_json,
        } => send(
            client,
            out,
            Method::PUT,
            &["notebook", &note_id, "paragraph", &paragraph_id, "config"],
            Some(input::json_arg(&config_json)?),
        ),
        ParagraphCmd::Move {
            note_id,
            paragraph_id,
            index,
        } => send(
            client,
            out,
            Method::POST,
            &[
                "notebook",
                &note_id,
                "paragraph",
                &paragraph_id,
                "move",
                &index.to_string(),
            ],
            None,
        ),
        ParagraphCmd::Delete {
            note_id,
            paragraph_id,
        } => send(
            client,
            out,
            Method::DELETE,
            &["notebook", &note_id, "paragraph", &paragraph_id],
            None,
        ),
        ParagraphCmd::Run {
            note_id,
            paragraph_id,
            sync,
            params,
            session_id,
            wait,
            wait_args,
        } => {
            let query = session_id
                .map(|s| vec![("sessionId", s)])
                .unwrap_or_default();
            let body = params_body(params)?.map(RequestBody::Json);
            if sync {
                return match client.call(
                    Method::POST,
                    &["notebook", "run", &note_id, &paragraph_id],
                    &query,
                    body,
                ) {
                    Ok(env) => {
                        out.response(&env);
                        let failed =
                            env.pointer("/body/code").and_then(Value::as_str) == Some("ERROR");
                        Ok(if failed { wait::EXIT_FAILED } else { 0 })
                    }
                    // The server answers a failed paragraph with HTTP 500 and the result in "body".
                    Err(e) => match e.downcast_ref::<ApiError>() {
                        Some(api) if api.body.pointer("/body/code").is_some() => {
                            out.response(&api.body);
                            eprintln!("paragraph {paragraph_id} failed");
                            Ok(wait::EXIT_FAILED)
                        }
                        _ => Err(e),
                    },
                };
            }
            let env = client.call(
                Method::POST,
                &["notebook", "job", &note_id, &paragraph_id],
                &query,
                body,
            )?;
            if wait {
                return wait::paragraph(client, out, &note_id, &paragraph_id, &wait_args);
            }
            out.response(&env);
            Ok(0)
        }
        ParagraphCmd::Stop {
            note_id,
            paragraph_id,
        } => send(
            client,
            out,
            Method::DELETE,
            &["notebook", "job", &note_id, &paragraph_id],
            None,
        ),
        ParagraphCmd::Status {
            note_id,
            paragraph_id,
        } => {
            out.response(&wait::StatusSource::default().paragraph(
                client,
                &note_id,
                &paragraph_id,
            )?);
            Ok(0)
        }
        ParagraphCmd::Wait {
            note_id,
            paragraph_id,
            wait_args,
        } => wait::paragraph(client, out, &note_id, &paragraph_id, &wait_args),
        ParagraphCmd::Output {
            note_id,
            paragraph_id,
        } => {
            let env = client.get(&["notebook", &note_id, "paragraph", &paragraph_id])?;
            if let Some(p) = env.get("body") {
                print_results(p);
            }
            Ok(0)
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn cli_definition_is_consistent() {
        use clap::CommandFactory;
        super::Cli::command().debug_assert();
    }
}
