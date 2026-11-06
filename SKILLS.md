---
name: zeppelin-cli
description: Drive Apache Zeppelin 0.11 notebooks from the shell with the `zeppelin` CLI. Covers creating notes, feeding in notebooks (JSON, stdin, paragraph files, exports), running them, polling for completion, reading output and cleaning up. Use it when asked to run, automate, test or inspect Zeppelin notebooks.
---

# Using the `zeppelin` CLI

`zeppelin` wraps the [Zeppelin 0.11.0 Notebook REST API](https://zeppelin.apache.org/docs/0.11.0/usage/rest_api/notebook.html).
Every API operation has a command, and there are a few helpers on top (`note wait`,
`note output`, `note id`).

## 1. Configure a connection

Connection details live in `~/.zeppelin.toml`. Run `zeppelin config example` to print a template:

```toml
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
username = "svc-zeppelin"      # overrides the top-level username/password
password = "change-me"
```

How the settings are chosen:

| Setting | Taken from (first match wins) |
|---|---|
| profile | `--profile/-p`, `$ZEPPELIN_PROFILE`, `default_profile`, a profile named `default`, the only profile |
| base URL | `--url`/`$ZEPPELIN_URL`, then the profile's `base_url` (`/api` is added for you) |
| username / password | `--username`/`--password` (or `$ZEPPELIN_USERNAME`/`$ZEPPELIN_PASSWORD`), then the profile, then the top-level values |

When a username is set, the CLI logs in with `POST /api/login` and reuses the session cookie.
Without one it calls the server anonymously.

Check the setup:

```bash
zeppelin config profiles      # list profiles (which one is default)
zeppelin config show          # resolved URL + user for this invocation (password hidden)
zeppelin ping                 # GET /api/version, which also checks the login
zeppelin -p prod ping         # same, against another profile
```

## 2. Conventions to rely on

- **stdout carries data and stderr carries progress and errors.** Responses print only their `body`.
  - A body that's a string, like a new note or paragraph id, prints bare, so `ID=$(zeppelin note create ...)` works.
  - Use `--raw` for the full `{status, message, body}` envelope.
  - Use `--compact` for single-line JSON, which is handy with `jq`.
- **JSON arguments** (`--params`, `--config`, `paragraph config`) take inline JSON, `@file.json`, or `-` for stdin.
- **Text and notebook inputs** (`--from`, `--paragraph-file`, `--text-file`, `note import`) take a path, or `-` for stdin.
- **Exit codes:**

  | Code | Meaning |
  |---|---|
  | 0 | success |
  | 1 | error (HTTP failure, bad input, login failed) |
  | 2 | invalid command-line usage |
  | 3 | a paragraph ended in `ERROR` or `ABORTED` (`wait`, `run --wait`, `paragraph run --sync`) |
  | 4 | `wait` timed out |

- Note **paths** look like `/folder/name`. Note **ids** look like `2ABCDEF12`. Use `zeppelin note id /folder/name` to turn a path into an id.

## 3. Worked example: create → run → poll → clean up

This is the full lifecycle in plain shell, polling by hand so every step is visible. It's also in
[`examples/run-notebook.sh`](examples/run-notebook.sh).

```bash
#!/usr/bin/env bash
set -euo pipefail

NAME="/tmp/cli-run-$(date +%s)-$$"          # unique path so parallel runs don't clash

# 1. Create a note with three paragraphs. The CLI prints only the new note id.
NOTE_ID=$(zeppelin note create --name "$NAME" \
  --paragraph $'%md\n# Nightly check' \
  --paragraph $'%sh\nfor i in 1 2 3 4 5; do echo "row,$i,$((i*i))"; done > /tmp/demo.csv\nwc -l /tmp/demo.csv' \
  --paragraph $'%sh\nawk -F, \'{ s += $3 } END { print "sum:", s }\' /tmp/demo.csv')
echo "created $NOTE_ID" >&2

# 4. Clean up no matter how the script exits.
trap 'zeppelin note delete "$NOTE_ID" >/dev/null && echo "deleted $NOTE_ID" >&2' EXIT

# 2. Start all paragraphs. This returns at once; the server runs them in the background.
zeppelin note run "$NOTE_ID"

# 3. Poll GET /notebook/job/{id} until the note is no longer running.
#    It's done when isRunning is false and no paragraph is PENDING or RUNNING.
deadline=$((SECONDS + 600))
while :; do
  status=$(zeppelin note status "$NOTE_ID" --compact)
  running=$(jq -r '.isRunning' <<<"$status")
  active=$(jq '[.paragraphs[] | select(.status=="PENDING" or .status=="RUNNING")] | length' <<<"$status")
  echo "isRunning=$running active=$active" >&2
  [[ $running == false && $active == 0 ]] && break
  (( SECONDS < deadline )) || { zeppelin note stop "$NOTE_ID"; echo "timed out" >&2; exit 4; }
  sleep 2
done

# Read the results as plain text.
zeppelin note output "$NOTE_ID"

# Fail the script if any paragraph failed.
if jq -e '[.paragraphs[] | select(.status=="ERROR" or .status=="ABORTED")] | length > 0' <<<"$status" >/dev/null; then
  jq -r '.paragraphs[] | select(.status=="ERROR") | "FAILED \(.id): \(.errorMessage // "")"' <<<"$status" >&2
  exit 3
fi
```

`note status` returns:

```json
{
  "id": "2N9A1NST8",
  "isRunning": false,
  "paragraphs": [
    {"id": "paragraph_..._91560879", "status": "FINISHED", "progress": "100", "started": "...", "finished": "..."},
    {"id": "paragraph_..._55800683", "status": "ERROR", "errorMessage": "ExitValue: 3", "...": "..."}
  ]
}
```

Paragraph statuses are `READY` (not run yet), `PENDING`, `RUNNING`, `FINISHED`, `ERROR` and `ABORTED`.
When a paragraph fails, the paragraphs after it stay `READY`.

### The same thing with the built-in poller

`note wait` runs the polling loop above. `note run --wait` starts the note and then waits:

```bash
NOTE_ID=$(zeppelin note create --from examples/notebooks/hello-shell.json --name /tmp/nightly)
trap 'zeppelin note delete "$NOTE_ID"' EXIT

zeppelin note run "$NOTE_ID" --wait --interval 2 --timeout 600 > status.json
case $? in
  0) zeppelin note output "$NOTE_ID" ;;
  3) echo "a paragraph failed"; jq '.paragraphs[] | select(.status=="ERROR")' status.json ;;
  4) echo "timed out"; zeppelin note stop "$NOTE_ID" ;;
esac
```

Note: with `set -e`, wrap the call (`if zeppelin note run ... --wait; then ...`) so exit code 3 or 4 doesn't end the script early.

`wait` prints progress to stderr when the state changes (hide it with `-q`):

```
[    0s] note 2N9A1NST8: running — 1 PENDING, 2 READY
[    2s] note 2N9A1NST8: running — 1 FINISHED, 1 RUNNING, 1 READY
[    5s] note 2N9A1NST8: idle — 3 FINISHED
```

Other ways to wait:

```bash
zeppelin note run "$NOTE_ID" --blocking          # server holds the HTTP request until the run ends
zeppelin note wait "$NOTE_ID" --timeout 300      # wait for a run someone else started
```

`--blocking` needs a `--http-timeout` longer than the run.

## 4. Feeding notebooks into the CLI

### a) A JSON notebook file

`--from` accepts `{"name": ..., "paragraphs": [...]}`, or just the array of paragraphs.
Each paragraph takes `text`, an optional `title` and an optional `config`:

```json
{
  "name": "/examples/hello-shell",
  "paragraphs": [
    {"title": "Intro", "text": "%md\n# Hello"},
    {"title": "Work",  "text": "%sh\necho working", "config": {"colWidth": 6.0}}
  ]
}
```

```bash
zeppelin note create --from examples/notebooks/hello-shell.json            # uses the file's name
zeppelin note create --from examples/notebooks/hello-shell.json --name /tmp/copy-1   # --name overrides it
zeppelin note create --from nb.json --default-interpreter python           # interpreter for paragraphs without %xxx
```

### b) Stdin: heredocs and generated notebooks

```bash
NOTE_ID=$(zeppelin note create --name /tmp/adhoc --from - <<'JSON'
{"paragraphs": [
  {"title": "query", "text": "%sh\necho from a heredoc"}
]}
JSON
)

# Generate paragraphs with jq, for example one per input file or date
jq -n '{name: "/tmp/backfill",
        paragraphs: [ ("2026-01-01","2026-01-02") | {title: ., text: "%sh\necho processing \(.)"} ]}' \
  | zeppelin note create --from - \
  | xargs -I{} zeppelin note run {} --wait -q
```

### c) One file per paragraph

Keep paragraph code in ordinary files under version control. Each file becomes one paragraph, in the
order given. The first line picks the interpreter:

```bash
cat examples/paragraphs/01-setup.sh
# %sh
# echo "setting up on $(hostname)"

zeppelin note create --name /tmp/from-files \
  --paragraph-file examples/paragraphs/01-setup.sh \
  --paragraph-file examples/paragraphs/02-report.md
```

`--from`, `--paragraph` and `--paragraph-file` can be mixed. Paragraphs are added in that order:
everything from `--from` first, then each `--paragraph`, then each `--paragraph-file`.

### d) Add paragraphs to an existing note

```bash
zeppelin paragraph create "$NOTE_ID" --title "extra" --text '%sh uname -a'
zeppelin paragraph create "$NOTE_ID" --text-file query.sql --index 0      # insert at the top
generate_code | zeppelin paragraph create "$NOTE_ID" --text-file -        # from a pipe
zeppelin paragraph create "$NOTE_ID" --text '%md hi' --config '{"colWidth": 6}'
```

### e) Import an exported note (`.zpln` / `.json`)

`note import` takes Zeppelin's own note format, the format `note export` produces and the UI downloads:

```bash
zeppelin note import my-note.zpln                          # keeps the name stored in the file
zeppelin note import my-note.zpln --name /team/my-note     # rename on the way in
cat my-note.zpln | zeppelin note import -                  # from stdin
```

Promote a note from one environment to another by piping one profile into another:

```bash
ID=$(zeppelin -p staging note id /team/report)
zeppelin -p staging note export "$ID" | zeppelin -p prod note import - --name /team/report
```

Export to a file for backups or git:

```bash
zeppelin note export "$ID" -o backups/report.zpln
```

Use `note create --from` for hand-written notebooks, and `note import` for full Zeppelin exports,
which include ids, results and settings.

## 5. More recipes

### Run one paragraph

```bash
PID=$(zeppelin note get "$NOTE_ID" | jq -r '.paragraphs[0].id')

zeppelin paragraph run "$NOTE_ID" "$PID" --sync          # blocks; prints {"code": "SUCCESS"|"ERROR", "msg": [...]}; exit 3 on ERROR
zeppelin paragraph run "$NOTE_ID" "$PID" --wait          # async run, then poll until done
zeppelin paragraph output "$NOTE_ID" "$PID"              # plain-text result
zeppelin paragraph stop "$NOTE_ID" "$PID"
```

### Dynamic form parameters

For paragraphs that use `${name=default}` style forms:

```bash
zeppelin paragraph run "$NOTE_ID" "$PID" --sync --params '{"date": "2026-11-01", "limit": "100"}'
zeppelin note run "$NOTE_ID" --wait --params @params.json
```

### Inspect and find notes

```bash
zeppelin note list --compact | jq -r '.[] | "\(.id)\t\(.path)"'
zeppelin note list --filter reports              # case-insensitive substring of the path
zeppelin note id "/team/daily report"            # path to id
zeppelin note get "$NOTE_ID" | jq '.paragraphs[] | {id, title, status}'
zeppelin note get --by-path /team/report
zeppelin search "select * from sales"            # full-text search across all paragraphs
```

### Edit notes and paragraphs

```bash
zeppelin paragraph update "$NOTE_ID" "$PID" --text-file new.sql --title "v2"
zeppelin paragraph config "$NOTE_ID" "$PID" '{"editorHide": true, "colWidth": 12}'
zeppelin paragraph move "$NOTE_ID" "$PID" 0
zeppelin paragraph delete "$NOTE_ID" "$PID"
zeppelin note rename "$NOTE_ID" /team/renamed
NEW=$(zeppelin note clone "$NOTE_ID" --name /team/copy)
zeppelin note clear "$NOTE_ID"                  # clear all paragraph output
```

### Schedules, permissions and revisions

```bash
zeppelin cron set "$NOTE_ID" "0 0 6 * * ?" --release-resource   # Quartz syntax
zeppelin cron get "$NOTE_ID"
zeppelin cron remove "$NOTE_ID"

zeppelin perms get "$NOTE_ID"
zeppelin perms set "$NOTE_ID" --owners alice --readers alice,bob --runners alice,bob --writers alice

zeppelin revision save "$NOTE_ID" -m "before refactor"
zeppelin revision list "$NOTE_ID"
zeppelin revision revert "$NOTE_ID" <revision-id>
```

`perms set` replaces all four lists, and any list you leave out is sent empty.

### Clean up everything a test run created

```bash
zeppelin note list --filter /tmp/cli-run- --compact | jq -r '.[].id' | xargs -r -n1 zeppelin note delete
```

## 6. Troubleshooting

| Symptom | Cause / fix |
|---|---|
| `login as 'x' failed (HTTP 401/403)` | Wrong credentials for the profile in use. Check `zeppelin config show`. |
| `HTTP 403 ... Insufficient privileges` | The logged-in user isn't an owner/writer/runner of that note. Check `zeppelin perms get`. |
| `HTTP 403 FORBIDDEN: Cron is not enabled` | The server needs `zeppelin.notebook.cron.enable=true`. |
| `revision` commands give `HTTP 404 ... non-JSON (HTML) response` | The server's notebook storage has no versioning (it needs a Git-backed notebook repo), or the server doesn't have these endpoints. |
| `note run` returns `HTTP 412 PRECONDITION_FAILED` | A paragraph's interpreter isn't bound or doesn't exist. Use a valid `%interpreter` or `--default-interpreter`. |
| `--blocking` / `--sync` time out | Raise `--http-timeout` (default 300 s), or use `--wait`, which polls instead. |
| `note status` works for finished notes but returned HTTP 500 for failed ones | Some builds, including the `apache/zeppelin:0.11.0` Docker image, crash on the job-status endpoint once a paragraph is in ERROR. `status` and `wait` fall back to reading the full note automatically, so the JSON shape stays the same. |
