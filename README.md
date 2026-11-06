# zeppelin-cli

A Rust command-line client for the [Apache Zeppelin 0.11.0 Notebook REST API](https://zeppelin.apache.org/docs/0.11.0/usage/rest_api/notebook.html).

```bash
cargo install --path .          # installs the `zeppelin` binary
zeppelin config example > ~/.zeppelin.toml   # then edit URLs and credentials
zeppelin ping
```

```bash
NOTE=$(zeppelin note create --from examples/notebooks/hello-shell.json --name /tmp/demo)
zeppelin note run "$NOTE" --wait && zeppelin note output "$NOTE"
zeppelin note delete "$NOTE"
```

- **Usage guide and worked examples:** [SKILLS.md](SKILLS.md). It has YAML front matter, so it can be dropped into a skills directory as-is.
- **Configuration:** `~/.zeppelin.toml` holds profiles (each with `base_url`, `username`, `password`) and top-level default credentials. Pick a profile with `-p NAME` or `ZEPPELIN_PROFILE`.
- **Command coverage:**
  - Notes: list, create, get, delete, clone, rename, import, export, run all, stop all, status, clear output.
  - Paragraphs: create, get, update, config, move, delete, run (async or sync), stop, status.
  - Also full-text search, cron, permissions and revisions.
  - Helpers: `note wait`, `paragraph wait`, `note output`, `paragraph output`, `note id`.

## Development

```bash
cargo test        # unit tests + end-to-end tests against a mock server
cargo clippy --all-targets
```

To try it against a real server:

```bash
docker run -d --name zeppelin -p 8080:8080 apache/zeppelin:0.11.0
zeppelin --url http://localhost:8080 note list
```
