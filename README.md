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
- **Configuration:** profiles in `~/.zeppelin.toml`; see [Configuration](#configuration).
- **Command coverage:**
  - Notes: list, create, get, delete, clone, rename, import, export, run all, stop all, status, clear output.
  - Paragraphs: create, get, update, config, move, delete, run (async or sync), stop, status.
  - Also full-text search, cron, permissions and revisions.
  - Helpers: `note wait`, `paragraph wait`, `note output`, `paragraph output`, `note id`.

## Configuration

Connection settings live in `~/.zeppelin.toml`. Each profile is one Zeppelin server. Username and
password at the top level are the defaults for every profile that doesn't set its own.

```toml
# ~/.zeppelin.toml

# Profile used when neither --profile nor ZEPPELIN_PROFILE is given.
default_profile = "local"

# Default credentials for every profile below.
username = "admin"
password = "password1"

[profiles.local]
base_url = "http://localhost:8080"          # /api is appended automatically

[profiles.staging]
base_url = "https://zeppelin.staging.example.com"
# no username/password here, so it uses the top-level ones

[profiles.prod]
base_url = "https://zeppelin.example.com/zeppelin"   # sub-paths work too
username = "svc-zeppelin"                   # overrides the top-level username
password = "change-me"
```

How each setting is chosen (first match wins):

| Setting | Order |
|---|---|
| Profile | `--profile`/`-p`, `ZEPPELIN_PROFILE`, `default_profile`, a profile named `default`, the only profile if there's just one |
| Server URL | `--url`/`ZEPPELIN_URL`, then the profile's `base_url` |
| Username / password | `--username`/`--password` (or `ZEPPELIN_USERNAME`/`ZEPPELIN_PASSWORD`), then the profile, then the top level |

With a username set, the CLI logs in through `POST /api/login` and reuses the session cookie.
Unknown keys are rejected, so a typo like `baseurl` is an error rather than silently ignored. Use
`--config PATH` or `ZEPPELIN_CONFIG` to read a different file.

```bash
zeppelin config example        # print a starter file
zeppelin config profiles       # list profiles and which one is the default
zeppelin -p prod config show   # the settings a command would use (password hidden)
zeppelin -p prod ping          # check the server is reachable and the login works
```

The file holds passwords, so make it readable only by you: `chmod 600 ~/.zeppelin.toml`.

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
