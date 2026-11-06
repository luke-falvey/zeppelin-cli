# zeppelin-cli

A Rust command-line client for the [Apache Zeppelin 0.11.0 Notebook REST API](https://zeppelin.apache.org/docs/0.11.0/usage/rest_api/notebook.html).

## Install

Download a prebuilt binary from the [latest release](https://github.com/luke-falvey/zeppelin-cli/releases/latest):

| Platform | Archive |
|---|---|
| Linux x86_64 (static) | `zeppelin-x86_64-unknown-linux-musl.tar.gz` |
| Linux ARM64 (static) | `zeppelin-aarch64-unknown-linux-musl.tar.gz` |
| macOS Apple Silicon | `zeppelin-aarch64-apple-darwin.tar.gz` |
| macOS Intel | `zeppelin-x86_64-apple-darwin.tar.gz` |
| Windows x86_64 | `zeppelin-x86_64-pc-windows-msvc.zip` |
| Windows ARM64 | `zeppelin-aarch64-pc-windows-msvc.zip` |

```bash
curl -fsSL https://github.com/luke-falvey/zeppelin-cli/releases/latest/download/zeppelin-x86_64-unknown-linux-musl.tar.gz | tar xz
sudo mv zeppelin-x86_64-unknown-linux-musl/zeppelin /usr/local/bin/
```

Each release includes a `SHA256SUMS` file. To build from source instead, run `cargo install --path .`.

## Quick start

```bash
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

CI (`.github/workflows/ci.yml`) runs on every push and pull request:
- `cargo fmt` and `clippy` checks.
- Tests on Linux, macOS and Windows.
- Release builds for all six platforms above, uploaded as workflow artifacts.

To publish a release, set `version` in `Cargo.toml`, commit, then push a matching tag. CI checks that the tag matches the version, then attaches the archives and checksums to a GitHub release. Tags with a hyphen (`v0.2.0-rc1`) become pre-releases.

```bash
git tag v0.1.0
git push origin v0.1.0
```

Local checks:

```bash
cargo test        # unit tests + end-to-end tests against a mock server
cargo clippy --all-targets
```

To try it against a real server:

```bash
docker run -d --name zeppelin -p 8080:8080 apache/zeppelin:0.11.0
zeppelin --url http://localhost:8080 note list
```
