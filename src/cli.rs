//! Command-line definitions.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "zeppelin",
    version,
    about = "Command-line client for the Apache Zeppelin 0.11 Notebook REST API",
    after_help = "Connection settings come from ~/.zeppelin.toml (see `zeppelin config example`).\n\
                  JSON arguments accept inline JSON, @file, or - for stdin.\n\n\
                  Exit codes: 0 success, 1 error, 2 usage error, 3 a paragraph ended in ERROR/ABORTED, 4 wait timed out."
)]
pub struct Cli {
    /// Profile from the config file to use.
    #[arg(short, long, global = true, env = "ZEPPELIN_PROFILE")]
    pub profile: Option<String>,

    /// Path to the config file.
    #[arg(long, global = true, env = "ZEPPELIN_CONFIG", value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Server URL, overriding the profile's base_url (e.g. http://localhost:8080).
    #[arg(long, global = true, env = "ZEPPELIN_URL")]
    pub url: Option<String>,

    /// Username, overriding the config file.
    #[arg(long, global = true, env = "ZEPPELIN_USERNAME")]
    pub username: Option<String>,

    /// Password, overriding the config file.
    #[arg(long, global = true, env = "ZEPPELIN_PASSWORD", hide_env_values = true)]
    pub password: Option<String>,

    /// Print the full response envelope ({status, message, body}) instead of just the body.
    #[arg(long, global = true)]
    pub raw: bool,

    /// Print JSON on a single line.
    #[arg(long, global = true)]
    pub compact: bool,

    /// HTTP request timeout in seconds.
    #[arg(long, global = true, default_value_t = 300, value_name = "SECS")]
    pub http_timeout: u64,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Notes (notebooks): list, create, import/export, run, poll status.
    #[command(subcommand, visible_alias = "notebook")]
    Note(NoteCmd),
    /// Paragraphs within a note.
    #[command(subcommand, visible_alias = "para")]
    Paragraph(ParagraphCmd),
    /// Full-text search through paragraphs in all notes.
    Search { query: String },
    /// Cron schedules for notes.
    #[command(subcommand)]
    Cron(CronCmd),
    /// Note permissions.
    #[command(subcommand, visible_alias = "perms")]
    Permissions(PermissionsCmd),
    /// Note revisions (version control).
    #[command(subcommand, visible_alias = "rev")]
    Revision(RevisionCmd),
    /// Inspect configuration and profiles.
    #[command(subcommand)]
    Config(ConfigCmd),
    /// Check connectivity and credentials (GET /api/version).
    Ping,
}

#[derive(Args, Clone)]
pub struct WaitArgs {
    /// Seconds between status polls.
    #[arg(long, default_value_t = 2.0, value_name = "SECS")]
    pub interval: f64,
    /// Give up after this many seconds (0 = wait forever). Exit code 4 on timeout.
    #[arg(long, default_value_t = 0.0, value_name = "SECS")]
    pub timeout: f64,
    /// Don't print progress to stderr.
    #[arg(short, long)]
    pub quiet: bool,
}

#[derive(Subcommand)]
pub enum NoteCmd {
    /// List notes (GET /notebook).
    #[command(visible_alias = "ls")]
    List {
        /// Only show notes whose path contains this text (case-insensitive).
        #[arg(long)]
        filter: Option<String>,
    },
    /// Create a note (POST /notebook). Prints the new note id.
    ///
    /// Paragraphs are added in this order: those in --from, then each --paragraph,
    /// then each --paragraph-file.
    Create {
        /// Note name/path, e.g. "/team/daily report".
        #[arg(short, long)]
        name: Option<String>,
        /// JSON file (or - for stdin) with {"name", "paragraphs": [{title, text, config}]},
        /// or just a JSON array of paragraphs.
        #[arg(long, value_name = "FILE|-")]
        from: Option<String>,
        /// Paragraph text to add (repeatable), e.g. --paragraph '%sh echo hi'.
        #[arg(long = "paragraph", value_name = "TEXT")]
        paragraphs: Vec<String>,
        /// File whose whole contents become one paragraph (repeatable; - for stdin).
        #[arg(long = "paragraph-file", value_name = "FILE|-")]
        paragraph_files: Vec<String>,
        /// Default interpreter group for the note (e.g. spark, python, sh).
        #[arg(long)]
        default_interpreter: Option<String>,
        /// Add an empty paragraph at the end (Zeppelin's UI default).
        #[arg(long)]
        add_empty_paragraph: bool,
    },
    /// Get a note with all its paragraphs (GET /notebook/{id}).
    Get {
        /// Note id (or path with --by-path).
        note: String,
        /// Treat NOTE as a note path like "/folder/name".
        #[arg(long)]
        by_path: bool,
        /// Ask the server to reload the note from storage.
        #[arg(long)]
        reload: bool,
    },
    /// Print the id of the note at a path (handy in scripts).
    Id {
        /// Note path, e.g. "/folder/name".
        path: String,
    },
    /// Delete a note (DELETE /notebook/{id}).
    #[command(visible_alias = "rm")]
    Delete { note_id: String },
    /// Clone a note (POST /notebook/{id}). Prints the new note id.
    Clone {
        note_id: String,
        #[arg(short, long)]
        name: Option<String>,
        /// Clone a specific revision.
        #[arg(long)]
        revision: Option<String>,
    },
    /// Rename a note (PUT /notebook/{id}/rename).
    #[command(visible_alias = "mv")]
    Rename { note_id: String, name: String },
    /// Export a note as Zeppelin JSON (GET /notebook/export/{id}).
    Export {
        note_id: String,
        /// Write to this file instead of stdout.
        #[arg(short, long, value_name = "FILE")]
        output: Option<PathBuf>,
    },
    /// Import a note from Zeppelin JSON (POST /notebook/import). Prints the new note id.
    Import {
        /// Note JSON file (.zpln / .json), or - for stdin.
        #[arg(default_value = "-", value_name = "FILE|-")]
        file: String,
        /// Override the note's name before importing.
        #[arg(short, long)]
        name: Option<String>,
        /// Import to this note path (notePath query parameter).
        #[arg(long)]
        path: Option<String>,
    },
    /// Run all paragraphs (POST /notebook/job/{id}).
    Run {
        note_id: String,
        /// Dynamic form parameters as JSON object (inline, @file or -).
        #[arg(long, value_name = "JSON")]
        params: Option<String>,
        /// Ask the server to block until the run finishes (blocking=true).
        #[arg(long, conflicts_with = "wait")]
        blocking: bool,
        /// Run in an isolated interpreter process (isolated=true).
        #[arg(long)]
        isolated: bool,
        /// After starting, poll until all paragraphs finish (like `note wait`).
        #[arg(short, long)]
        wait: bool,
        #[command(flatten)]
        wait_args: WaitArgs,
    },
    /// Stop all paragraphs (DELETE /notebook/job/{id}).
    Stop { note_id: String },
    /// Status of all paragraphs (GET /notebook/job/{id}).
    ///
    /// Prints {"id", "isRunning", "paragraphs": [{id, status, progress, started, finished, errorMessage}]}.
    Status { note_id: String },
    /// Poll the note's status until it is no longer running.
    ///
    /// Exits 0 when every paragraph finished, 3 if any ended in ERROR/ABORTED, 4 on timeout.
    Wait {
        note_id: String,
        #[command(flatten)]
        wait_args: WaitArgs,
    },
    /// Print every paragraph's output as plain text.
    Output { note_id: String },
    /// Clear all paragraph output (PUT /notebook/{id}/clear).
    Clear { note_id: String },
}

#[derive(Args)]
pub struct TextArgs {
    /// Paragraph text, e.g. '%python print(1)'.
    #[arg(long, conflicts_with = "text_file")]
    pub text: Option<String>,
    /// Read paragraph text from a file (- for stdin).
    #[arg(long, value_name = "FILE|-")]
    pub text_file: Option<String>,
    /// Paragraph title.
    #[arg(long)]
    pub title: Option<String>,
}

#[derive(Subcommand)]
pub enum ParagraphCmd {
    /// Add a paragraph (POST /notebook/{id}/paragraph). Prints the new paragraph id.
    #[command(visible_alias = "add")]
    Create {
        note_id: String,
        #[command(flatten)]
        text: TextArgs,
        /// Insert at this index (default: append).
        #[arg(long)]
        index: Option<u32>,
        /// Paragraph config as JSON (inline, @file or -).
        #[arg(long, value_name = "JSON")]
        config: Option<String>,
    },
    /// Get a paragraph (GET /notebook/{id}/paragraph/{pid}).
    Get {
        note_id: String,
        paragraph_id: String,
    },
    /// Update a paragraph's text and/or title (PUT /notebook/{id}/paragraph/{pid}).
    Update {
        note_id: String,
        paragraph_id: String,
        #[command(flatten)]
        text: TextArgs,
    },
    /// Update a paragraph's config (PUT /notebook/{id}/paragraph/{pid}/config).
    Config {
        note_id: String,
        paragraph_id: String,
        /// Config fields as JSON (inline, @file or -), e.g. '{"colWidth": 6}'.
        #[arg(value_name = "JSON")]
        config_json: String,
    },
    /// Move a paragraph (POST /notebook/{id}/paragraph/{pid}/move/{index}).
    Move {
        note_id: String,
        paragraph_id: String,
        index: u32,
    },
    /// Delete a paragraph (DELETE /notebook/{id}/paragraph/{pid}).
    #[command(visible_alias = "rm")]
    Delete {
        note_id: String,
        paragraph_id: String,
    },
    /// Run a paragraph. Asynchronous by default (POST /notebook/job/{id}/{pid});
    /// --sync uses POST /notebook/run/{id}/{pid} and prints the result.
    Run {
        note_id: String,
        paragraph_id: String,
        /// Run synchronously and print the result; exits 3 if the paragraph errors.
        #[arg(long, conflicts_with = "wait")]
        sync: bool,
        /// Dynamic form parameters as JSON object (inline, @file or -).
        #[arg(long, value_name = "JSON")]
        params: Option<String>,
        /// Interpreter session id (sessionId query parameter).
        #[arg(long)]
        session_id: Option<String>,
        /// After starting, poll until the paragraph finishes.
        #[arg(short, long)]
        wait: bool,
        #[command(flatten)]
        wait_args: WaitArgs,
    },
    /// Stop a paragraph (DELETE /notebook/job/{id}/{pid}).
    Stop {
        note_id: String,
        paragraph_id: String,
    },
    /// Status of one paragraph (GET /notebook/job/{id}/{pid}).
    Status {
        note_id: String,
        paragraph_id: String,
    },
    /// Poll until the paragraph is no longer PENDING/RUNNING.
    Wait {
        note_id: String,
        paragraph_id: String,
        #[command(flatten)]
        wait_args: WaitArgs,
    },
    /// Print a paragraph's output as plain text.
    Output {
        note_id: String,
        paragraph_id: String,
    },
}

#[derive(Subcommand)]
pub enum CronCmd {
    /// Get a note's cron job (GET /notebook/cron/{id}).
    Get { note_id: String },
    /// Set a note's cron job (POST /notebook/cron/{id}).
    #[command(visible_alias = "add")]
    Set {
        note_id: String,
        /// Quartz cron expression, e.g. "0 0/5 * * * ?".
        cron: String,
        /// Release interpreter resources after each run.
        #[arg(long)]
        release_resource: bool,
    },
    /// Remove a note's cron job (DELETE /notebook/cron/{id}).
    #[command(visible_alias = "rm")]
    Remove { note_id: String },
}

#[derive(Subcommand)]
pub enum PermissionsCmd {
    /// Get a note's permissions (GET /notebook/{id}/permissions).
    Get { note_id: String },
    /// Set a note's permissions (PUT /notebook/{id}/permissions).
    ///
    /// Each list is comma-separated. Lists you leave out are sent as empty.
    Set {
        note_id: String,
        #[arg(long, value_delimiter = ',')]
        owners: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        readers: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        runners: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        writers: Vec<String>,
    },
}

#[derive(Subcommand)]
pub enum RevisionCmd {
    /// List a note's revisions (GET /notebook/{id}/revision).
    #[command(visible_alias = "ls")]
    List { note_id: String },
    /// Save a revision (POST /notebook/{id}/revision). Prints the revision id.
    Save {
        note_id: String,
        #[arg(short, long)]
        message: String,
    },
    /// Get a note at a revision (GET /notebook/{id}/revision/{rid}).
    Get {
        note_id: String,
        revision_id: String,
    },
    /// Revert a note to a revision (PUT /notebook/{id}/revision/{rid}).
    Revert {
        note_id: String,
        revision_id: String,
    },
}

#[derive(Subcommand)]
pub enum ConfigCmd {
    /// Show the resolved connection settings (password hidden).
    Show,
    /// List the profiles in the config file.
    Profiles,
    /// Print the config file path.
    Path,
    /// Print an example ~/.zeppelin.toml.
    Example,
}
