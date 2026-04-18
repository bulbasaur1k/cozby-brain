//! cozby — console UI for cozby-brain. Talks to the HTTP API.

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Duration, Utc};
use clap::{Parser, Subcommand};
use serde::Serialize;
use serde_json::Value;

#[derive(Parser)]
#[command(
    name = "cozby",
    version,
    about = "Console UI for cozby-brain",
    subcommand_required = false,
    arg_required_else_help = false
)]
struct Cli {
    /// Base URL of the cozby-brain HTTP API
    #[arg(long, env = "COZBY_API", default_value = "http://localhost:8081")]
    api: String,

    /// Subcommand. Omit to enter interactive mode.
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Notes (markdown)
    #[command(subcommand)]
    Note(NoteCmd),
    /// Todo list
    #[command(subcommand)]
    Todo(TodoCmd),
    /// Reminders (fired via notifier channels)
    #[command(subcommand)]
    Remind(RemindCmd),
    /// LLM-powered ingestion: write anything in natural language.
    /// LLM classifies (note/todo/reminder/question) and structures it.
    Ingest {
        /// Inline text. If omitted — reads from --file, then stdin.
        #[arg(long)]
        text: Option<String>,
        /// Read from a text/markdown file.
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Smart search across notes, todos and reminders (LLM-assisted).
    Ask {
        /// Natural-language query.
        query: String,
    },
    /// Learning tracks — split a file into daily lessons via LLM.
    #[command(subcommand)]
    Learn(LearnCmd),
    /// Show ASCII graph of connections (semantic + wiki-links) for a note.
    Graph {
        /// Note id (full or prefix).
        id: String,
        /// Graph depth (1-3).
        #[arg(long, default_value = "1")]
        depth: u8,
    },
}

#[derive(Subcommand)]
enum LearnCmd {
    /// Create a new learning track from a file. LLM splits into lessons.
    Add {
        /// Path to the source file (txt/md/llm.txt).
        file: PathBuf,
        /// Title of the track (e.g. "Rust async", "English B1").
        #[arg(long)]
        title: String,
        /// How often to deliver new lessons (in hours).
        #[arg(long, default_value = "24")]
        pace: i32,
        /// Comma-separated tags.
        #[arg(long, value_delimiter = ',')]
        tags: Vec<String>,
    },
    /// List all tracks.
    List,
    /// List lessons in a track.
    Lessons { track_id: String },
    /// Deliver the next pending lesson manually.
    Next { track_id: String },
    /// Mark a lesson as learned.
    Learned { lesson_id: String },
    /// Skip a lesson.
    Skip { lesson_id: String },
    /// Delete a track (and all its lessons).
    Rm { track_id: String },
}

#[derive(Subcommand)]
enum NoteCmd {
    /// Add a note. Content comes from --content, --file, or stdin.
    Add {
        #[arg(long)]
        title: Option<String>,
        /// Read content from a text/markdown file.
        #[arg(long)]
        file: Option<PathBuf>,
        /// Inline content (takes precedence over stdin).
        #[arg(long)]
        content: Option<String>,
        /// Comma-separated tags.
        #[arg(long, value_delimiter = ',')]
        tags: Vec<String>,
    },
    /// List all notes.
    List,
    /// Show a note by id.
    Show { id: String },
    /// Full-text search.
    Search { query: String },
    /// Delete a note.
    Rm { id: String },
}

#[derive(Subcommand)]
enum TodoCmd {
    /// Add a todo.
    Add {
        title: String,
        /// Optional due date, RFC3339 (e.g. 2026-05-01T09:00:00Z) or relative like +30m, +2h, +1d.
        #[arg(long)]
        due: Option<String>,
    },
    /// List todos.
    List,
    /// Mark todo as done.
    Done { id: String },
    /// Delete a todo.
    Rm { id: String },
}

#[derive(Subcommand)]
enum RemindCmd {
    /// Add a reminder.
    Add {
        text: String,
        /// When to fire. RFC3339 or relative: +30m, +2h, +1d.
        #[arg(long)]
        at: String,
    },
    /// List reminders.
    List,
    /// Delete a reminder.
    Rm { id: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let client = Client::new(cli.api);
    match cli.cmd {
        None => interactive(&client).await,
        Some(Cmd::Note(c)) => note_cmd(&client, c).await,
        Some(Cmd::Todo(c)) => todo_cmd(&client, c).await,
        Some(Cmd::Remind(c)) => remind_cmd(&client, c).await,
        Some(Cmd::Ingest { text, file }) => ingest_universal(&client, text, file).await,
        Some(Cmd::Ask { query }) => ask_cmd(&client, &query).await,
        Some(Cmd::Learn(c)) => learn_cmd(&client, c).await,
        Some(Cmd::Graph { id, depth }) => graph_cmd(&client, &id, depth).await,
    }
}

// ------------------------ interactive mode ------------------------

async fn interactive(cli: &Client) -> Result<()> {
    use dialoguer::{theme::ColorfulTheme, Editor, Input, Select};

    println!("cozby — interactive mode");
    println!("api: {}", cli.base);
    println!("(press Ctrl+C to exit)\n");

    let theme = ColorfulTheme::default();
    let items = &[
        "📝 Ingest note   — paste raw text, LLM structures it",
        "✅ Ingest todo   — natural language ('купить молоко завтра в 10')",
        "⏰ Ingest remind — natural language ('через 30 минут позвонить маме')",
        "🔍 Ask           — smart search across everything",
        "➕ Add note      — manual (title + editor, no LLM)",
        "📋 List notes",
        "📋 List todos",
        "📋 List reminders",
        "🚪 Quit",
    ];

    loop {
        println!();
        let sel = Select::with_theme(&theme)
            .with_prompt("what now?")
            .default(0)
            .items(items)
            .interact()?;

        match sel {
            0 => {
                // Ingest note — opens $EDITOR for free-form input (perfect for big pastes)
                let template = "<!-- write/paste freely; LLM will structure this into a markdown note -->\n\n";
                let raw = Editor::new().extension(".md").edit(template)?;
                let Some(raw) = raw else {
                    println!("(cancelled)");
                    continue;
                };
                let cleaned = strip_template_comments(&raw);
                if cleaned.trim().is_empty() {
                    println!("(empty, skipped)");
                    continue;
                }
                ingest_universal(cli, Some(cleaned), None).await?;
            }
            1 => {
                let text: String = Input::with_theme(&theme)
                    .with_prompt("todo (natural language)")
                    .interact_text()?;
                if !text.trim().is_empty() {
                    ingest_universal(cli, Some(text), None).await?;
                }
            }
            2 => {
                let text: String = Input::with_theme(&theme)
                    .with_prompt("reminder (natural language)")
                    .interact_text()?;
                if !text.trim().is_empty() {
                    ingest_universal(cli, Some(text), None).await?;
                }
            }
            3 => {
                let q: String = Input::with_theme(&theme)
                    .with_prompt("search query")
                    .interact_text()?;
                if !q.trim().is_empty() {
                    ask_cmd(cli, &q).await?;
                }
            }
            4 => {
                let title: String = Input::with_theme(&theme)
                    .with_prompt("title")
                    .interact_text()?;
                let content = Editor::new()
                    .extension(".md")
                    .edit("")?
                    .unwrap_or_default();
                let tags_s: String = Input::with_theme(&theme)
                    .with_prompt("tags (comma-separated, empty for none)")
                    .allow_empty(true)
                    .interact_text()?;
                let tags: Vec<String> = tags_s
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                note_cmd(
                    cli,
                    NoteCmd::Add {
                        title: Some(title),
                        file: None,
                        content: Some(content),
                        tags,
                    },
                )
                .await?;
            }
            5 => note_cmd(cli, NoteCmd::List).await?,
            6 => todo_cmd(cli, TodoCmd::List).await?,
            7 => remind_cmd(cli, RemindCmd::List).await?,
            _ => {
                println!("bye");
                break;
            }
        }
    }
    Ok(())
}

fn strip_template_comments(s: &str) -> String {
    s.lines()
        .filter(|l| !l.trim_start().starts_with("<!--"))
        .collect::<Vec<_>>()
        .join("\n")
}

// ------------------------ ingest / ask ------------------------

/// Universal ingest: sends raw text to /api/ingest, LLM classifies (note/todo/reminder/question).
/// For notes — triggers 2-step flow with suggestion prompt.
async fn ingest_universal(
    cli: &Client,
    text: Option<String>,
    file: Option<PathBuf>,
) -> Result<()> {
    let raw = resolve_ingest_input(text, file)?;
    if raw.trim().is_empty() {
        println!("(пусто, пропущено)");
        return Ok(());
    }
    #[derive(Serialize)]
    struct Req<'a> {
        raw: &'a str,
    }
    let resp = cli.post("/api/ingest", &Req { raw: &raw }).await?;
    let kind = resp["type"].as_str().unwrap_or("");
    match kind {
        "note" => {
            let structured = resp["structured"].clone();
            let suggestion = resp["suggestion"].clone();
            confirm_note_flow(cli, structured, suggestion).await?;
        }
        "todo" => {
            let t = &resp["data"];
            let due = t["due_at"].as_str().unwrap_or("-");
            println!("создан todo: {}  {}  (due: {})", t["id"], t["title"], due);
        }
        "reminder" => {
            let r = &resp["data"];
            println!(
                "назначен reminder: {}  \"{}\"  @ {}",
                r["id"], r["text"], r["remind_at"]
            );
        }
        "question" => {
            let kw = resp["keywords"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            let scope = resp["scope"].as_str().unwrap_or("all");
            println!("поиск по ключевым: [{kw}]  scope={scope}");
            print_search_results(&resp["data"]);
        }
        other => {
            println!("неизвестный тип: {other}");
            println!("{}", serde_json::to_string_pretty(&resp).unwrap_or_default());
        }
    }
    Ok(())
}

fn resolve_ingest_input(text: Option<String>, file: Option<PathBuf>) -> Result<String> {
    if let Some(t) = text {
        return Ok(t);
    }
    if let Some(p) = file {
        return std::fs::read_to_string(&p).with_context(|| format!("read {}", p.display()));
    }
    use std::io::Read;
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;
    Ok(buf)
}

fn print_search_results(data: &Value) {
    let notes = data["notes"].as_array().cloned().unwrap_or_default();
    let todos = data["todos"].as_array().cloned().unwrap_or_default();
    let reminders = data["reminders"].as_array().cloned().unwrap_or_default();
    if !notes.is_empty() {
        println!("── notes ──");
        print_notes_table(&Value::Array(notes.clone()));
    }
    if !todos.is_empty() {
        println!("── todos ──");
        for t in &todos {
            let mark = if t["done"].as_bool().unwrap_or(false) {
                "[x]"
            } else {
                "[ ]"
            };
            println!("{} {}  {}", mark, t["id"], t["title"]);
        }
    }
    if !reminders.is_empty() {
        println!("── reminders ──");
        for r in &reminders {
            println!("· {}  \"{}\"  @ {}", r["id"], r["text"], r["remind_at"]);
        }
    }
    if notes.is_empty() && todos.is_empty() && reminders.is_empty() {
        println!("(ничего не найдено)");
    }
}

/// 2-step note confirmation: show structured + maybe suggestion → user chooses create/append.
async fn confirm_note_flow(
    cli: &Client,
    structured: Value,
    suggestion: Value,
) -> Result<()> {
    use dialoguer::{theme::ColorfulTheme, Select};

    let title = structured["title"].as_str().unwrap_or("untitled");
    let content = structured["content"].as_str().unwrap_or("");
    let tags: Vec<&str> = structured["tags"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    println!("Структурировано: \"{title}\"");
    if !tags.is_empty() {
        println!("Теги: {}", tags.join(", "));
    }

    let action;
    let target_id;
    if suggestion.is_object() && !suggestion.is_null() {
        let s_title = suggestion["target_title"].as_str().unwrap_or("?");
        let s_score = suggestion["score"].as_f64().unwrap_or(0.0);
        let s_reason = suggestion["reason"].as_str().unwrap_or("");
        let s_id = suggestion["target_id"].as_str().unwrap_or("");

        println!();
        println!("⚠ Найдена похожая заметка: \"{s_title}\" (score: {s_score:.2})");
        if !s_reason.is_empty() {
            println!("  Причина: {s_reason}");
        }
        println!();

        let theme = ColorfulTheme::default();
        let items = &[
            format!("Дополнить \"{s_title}\""),
            "Создать новую заметку".to_string(),
            "Отмена".to_string(),
        ];
        let sel = Select::with_theme(&theme)
            .with_prompt("Что делать?")
            .items(items)
            .default(0)
            .interact()?;
        match sel {
            0 => {
                action = "append";
                target_id = Some(s_id.to_string());
            }
            1 => {
                action = "create";
                target_id = None;
            }
            _ => {
                println!("(отменено)");
                return Ok(());
            }
        }
    } else {
        action = "create";
        target_id = None;
    }

    #[derive(Serialize)]
    struct Confirm<'a> {
        action: &'a str,
        target_id: Option<String>,
        title: &'a str,
        content: &'a str,
        tags: Vec<String>,
    }
    let resp = cli
        .post(
            "/api/ingest/note/confirm",
            &Confirm {
                action,
                target_id,
                title,
                content,
                tags: tags.iter().map(|s| s.to_string()).collect(),
            },
        )
        .await?;
    let n = &resp["data"];
    if action == "append" {
        println!("дополнено: {}  {}", n["id"], n["title"]);
    } else {
        println!("создано: {}  {}", n["id"], n["title"]);
    }
    Ok(())
}

async fn ask_cmd(cli: &Client, query: &str) -> Result<()> {
    let resp = cli
        .get(&format!("/api/ask?q={}", urlencode(query)))
        .await?;
    if let Some(kw) = resp["keywords"].as_array() {
        let s: Vec<_> = kw.iter().filter_map(|v| v.as_str()).collect();
        if !s.is_empty() {
            println!("keywords: {}", s.join(", "));
            println!();
        }
    }
    let d = &resp["data"];
    let notes = d["notes"].as_array().cloned().unwrap_or_default();
    let todos = d["todos"].as_array().cloned().unwrap_or_default();
    let reminders = d["reminders"].as_array().cloned().unwrap_or_default();
    let notes_empty = notes.is_empty();
    if !notes_empty {
        println!("── notes ──");
        print_notes_table(&Value::Array(notes));
        println!();
    }
    if !todos.is_empty() {
        println!("── todos ──");
        for t in &todos {
            let mark = if t["done"].as_bool().unwrap_or(false) { "[x]" } else { "[ ]" };
            println!("{} {}  {}", mark, t["id"], t["title"]);
        }
        println!();
    }
    if !reminders.is_empty() {
        println!("── reminders ──");
        for r in &reminders {
            println!("· {}  \"{}\"  @ {}", r["id"], r["text"], r["remind_at"]);
        }
        println!();
    }
    if notes_empty && todos.is_empty() && reminders.is_empty() {
        println!("(nothing matched)");
    }
    Ok(())
}

// ------------------------ note commands ------------------------

async fn note_cmd(cli: &Client, c: NoteCmd) -> Result<()> {
    match c {
        NoteCmd::Add { title, file, content, tags } => {
            let (title, body) = resolve_note_input(title, file, content)?;
            #[derive(Serialize)]
            struct Req {
                title: String,
                content: String,
                tags: Vec<String>,
            }
            let resp = cli.post("/api/notes", &Req { title, content: body, tags }).await?;
            let note = &resp["data"];
            println!("created: {}  {}", note["id"], note["title"]);
        }
        NoteCmd::List => {
            let resp = cli.get("/api/notes").await?;
            print_notes_table(&resp["data"]);
        }
        NoteCmd::Show { id } => {
            let resp = cli.get(&format!("/api/notes/{id}")).await?;
            let n = &resp["data"];
            println!("# {}", n["title"].as_str().unwrap_or(""));
            println!("id: {}", n["id"].as_str().unwrap_or(""));
            if let Some(tags) = n["tags"].as_array() {
                let t: Vec<_> = tags.iter().filter_map(|v| v.as_str()).collect();
                if !t.is_empty() {
                    println!("tags: {}", t.join(", "));
                }
            }
            if let Some(links) = resp["links"].as_array() {
                let l: Vec<_> = links.iter().filter_map(|v| v.as_str()).collect();
                if !l.is_empty() {
                    println!("links: {}", l.join(", "));
                }
            }
            println!("updated: {}", n["updated_at"].as_str().unwrap_or(""));
            println!();
            println!("{}", n["content"].as_str().unwrap_or(""));
        }
        NoteCmd::Search { query } => {
            let resp = cli
                .get(&format!("/api/notes/search?q={}", urlencode(&query)))
                .await?;
            print_notes_table(&resp["data"]);
        }
        NoteCmd::Rm { id } => {
            cli.delete(&format!("/api/notes/{id}")).await?;
            println!("deleted: {id}");
        }
    }
    Ok(())
}

fn resolve_note_input(
    title: Option<String>,
    file: Option<PathBuf>,
    content: Option<String>,
) -> Result<(String, String)> {
    // Content priority: --content > --file > stdin
    let (content, derived_title) = if let Some(c) = content {
        (c, None)
    } else if let Some(ref p) = file {
        let body = std::fs::read_to_string(p)
            .with_context(|| format!("read {}", p.display()))?;
        let stem = p
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string());
        (body, stem)
    } else {
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        (buf, None)
    };
    let title = title
        .or(derived_title)
        .or_else(|| first_line_title(&content))
        .unwrap_or_else(|| "untitled".to_string());
    Ok((title, content))
}

fn first_line_title(s: &str) -> Option<String> {
    s.lines()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim_start_matches('#').trim().to_string())
        .filter(|s| !s.is_empty())
}

fn print_notes_table(data: &Value) {
    let Some(arr) = data.as_array() else {
        println!("(empty)");
        return;
    };
    if arr.is_empty() {
        println!("(no notes)");
        return;
    }
    for n in arr {
        let id = n["id"].as_str().unwrap_or("");
        let title = n["title"].as_str().unwrap_or("");
        let updated = n["updated_at"].as_str().unwrap_or("");
        let tags = n["tags"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_default();
        println!("{}  {:<40}  [{}]  {}", &id[..8.min(id.len())], title, tags, updated);
    }
}

// ------------------------ todo commands ------------------------

async fn todo_cmd(cli: &Client, c: TodoCmd) -> Result<()> {
    match c {
        TodoCmd::Add { title, due } => {
            #[derive(Serialize)]
            struct Req {
                title: String,
                due_at: Option<DateTime<Utc>>,
            }
            let due_at = due.map(|s| parse_time(&s)).transpose()?;
            let resp = cli.post("/api/todos", &Req { title, due_at }).await?;
            let t = &resp["data"];
            println!("created: {}  {}", t["id"], t["title"]);
        }
        TodoCmd::List => {
            let resp = cli.get("/api/todos").await?;
            let Some(arr) = resp["data"].as_array() else {
                println!("(empty)");
                return Ok(());
            };
            if arr.is_empty() {
                println!("(no todos)");
                return Ok(());
            }
            for t in arr {
                let mark = if t["done"].as_bool().unwrap_or(false) {
                    "[x]"
                } else {
                    "[ ]"
                };
                let id = t["id"].as_str().unwrap_or("");
                let title = t["title"].as_str().unwrap_or("");
                let due = t["due_at"].as_str().unwrap_or("");
                println!("{} {}  {:<40}  {}", mark, &id[..8.min(id.len())], title, due);
            }
        }
        TodoCmd::Done { id } => {
            cli.post_empty(&format!("/api/todos/{id}/complete")).await?;
            println!("done: {id}");
        }
        TodoCmd::Rm { id } => {
            cli.delete(&format!("/api/todos/{id}")).await?;
            println!("deleted: {id}");
        }
    }
    Ok(())
}

// ------------------------ reminder commands ------------------------

async fn remind_cmd(cli: &Client, c: RemindCmd) -> Result<()> {
    match c {
        RemindCmd::Add { text, at } => {
            #[derive(Serialize)]
            struct Req {
                text: String,
                remind_at: DateTime<Utc>,
            }
            let remind_at = parse_time(&at)?;
            let resp = cli.post("/api/reminders", &Req { text, remind_at }).await?;
            let r = &resp["data"];
            println!("scheduled: {}  at {}", r["id"], r["remind_at"]);
        }
        RemindCmd::List => {
            let resp = cli.get("/api/reminders").await?;
            let Some(arr) = resp["data"].as_array() else {
                println!("(empty)");
                return Ok(());
            };
            if arr.is_empty() {
                println!("(no reminders)");
                return Ok(());
            }
            for r in arr {
                let mark = if r["fired"].as_bool().unwrap_or(false) {
                    "✓"
                } else {
                    "·"
                };
                let id = r["id"].as_str().unwrap_or("");
                let text = r["text"].as_str().unwrap_or("");
                let at = r["remind_at"].as_str().unwrap_or("");
                println!("{} {}  {:<50}  @ {}", mark, &id[..8.min(id.len())], text, at);
            }
        }
        RemindCmd::Rm { id } => {
            cli.delete(&format!("/api/reminders/{id}")).await?;
            println!("deleted: {id}");
        }
    }
    Ok(())
}

// ------------------------ helpers ------------------------

/// Parses RFC3339 timestamp or relative expressions like `+30m`, `+2h`, `+1d`.
fn parse_time(s: &str) -> Result<DateTime<Utc>> {
    if let Some(rest) = s.strip_prefix('+') {
        let (num, unit) = rest.split_at(rest.len().saturating_sub(1));
        let n: i64 = num.parse().context("parse relative time number")?;
        let delta = match unit {
            "s" => Duration::seconds(n),
            "m" => Duration::minutes(n),
            "h" => Duration::hours(n),
            "d" => Duration::days(n),
            _ => bail!("unit must be s/m/h/d"),
        };
        return Ok(Utc::now() + delta);
    }
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .context("parse rfc3339 timestamp")
}

fn urlencode(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
                c.to_string()
            } else {
                format!("%{:02X}", c as u32)
            }
        })
        .collect()
}

struct Client {
    base: String,
    http: reqwest::Client,
}

impl Client {
    fn new(base: String) -> Self {
        Self {
            base: base.trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
        }
    }

    async fn get(&self, path: &str) -> Result<Value> {
        let url = format!("{}{}", self.base, path);
        let resp = self.http.get(&url).send().await?;
        unwrap_json(resp).await
    }

    async fn post<T: Serialize>(&self, path: &str, body: &T) -> Result<Value> {
        let url = format!("{}{}", self.base, path);
        let resp = self.http.post(&url).json(body).send().await?;
        unwrap_json(resp).await
    }

    async fn post_empty(&self, path: &str) -> Result<Value> {
        let url = format!("{}{}", self.base, path);
        let resp = self.http.post(&url).send().await?;
        unwrap_json(resp).await
    }

    async fn delete(&self, path: &str) -> Result<Value> {
        let url = format!("{}{}", self.base, path);
        let resp = self.http.delete(&url).send().await?;
        unwrap_json(resp).await
    }
}

async fn unwrap_json(resp: reqwest::Response) -> Result<Value> {
    let status = resp.status();
    let body: Value = resp.json().await.context("decode json")?;
    if !status.is_success() {
        let msg = body["error"].as_str().unwrap_or("request failed").to_string();
        bail!("[{status}] {msg}");
    }
    Ok(body)
}

// ------------------------ learning commands ------------------------

async fn learn_cmd(cli: &Client, c: LearnCmd) -> Result<()> {
    match c {
        LearnCmd::Add {
            file,
            title,
            pace,
            tags,
        } => {
            let raw_text = std::fs::read_to_string(&file)
                .with_context(|| format!("read {}", file.display()))?;
            #[derive(Serialize)]
            struct Req {
                title: String,
                raw_text: String,
                file_path: String,
                pace_hours: i32,
                tags: Vec<String>,
            }
            println!("загружаю {} ({} символов)...", file.display(), raw_text.len());
            println!("LLM разбивает материал на уроки — это может занять минуту...");
            let resp = cli
                .post(
                    "/api/learning/tracks",
                    &Req {
                        title,
                        raw_text,
                        file_path: file.display().to_string(),
                        pace_hours: pace,
                        tags,
                    },
                )
                .await?;
            let t = &resp["data"];
            println!(
                "создан трек: {}  \"{}\"  уроков: {}  темп: раз в {} ч",
                t["id"], t["title"], t["total_lessons"], t["pace_hours"]
            );
        }
        LearnCmd::List => {
            let resp = cli.get("/api/learning/tracks").await?;
            let Some(arr) = resp["data"].as_array() else {
                println!("(нет треков)");
                return Ok(());
            };
            if arr.is_empty() {
                println!("(нет треков)");
                return Ok(());
            }
            for t in arr {
                let id = t["id"].as_str().unwrap_or("");
                let title = t["title"].as_str().unwrap_or("");
                let cur = t["current_lesson"].as_i64().unwrap_or(0);
                let total = t["total_lessons"].as_i64().unwrap_or(0);
                let pace = t["pace_hours"].as_i64().unwrap_or(0);
                println!(
                    "{}  {:<30}  {:>3}/{:<3}  каждые {} ч",
                    &id[..8.min(id.len())],
                    title,
                    cur,
                    total,
                    pace
                );
            }
        }
        LearnCmd::Lessons { track_id } => {
            let resp = cli
                .get(&format!("/api/learning/tracks/{track_id}/lessons"))
                .await?;
            let Some(arr) = resp["data"].as_array() else {
                println!("(нет уроков)");
                return Ok(());
            };
            for l in arr {
                let status = l["status"].as_str().unwrap_or("?");
                let num = l["lesson_num"].as_i64().unwrap_or(0);
                let title = l["title"].as_str().unwrap_or("");
                let id = l["id"].as_str().unwrap_or("");
                let mark = match status {
                    "learned" => "[✓]",
                    "skipped" => "[—]",
                    "delivered" => "[▶]",
                    _ => "[ ]",
                };
                println!(
                    "{} #{:>3}  {}  {:<40}  [{}]",
                    mark,
                    num,
                    &id[..8.min(id.len())],
                    title,
                    status
                );
            }
        }
        LearnCmd::Next { track_id } => {
            let resp = cli
                .post_empty(&format!("/api/learning/tracks/{track_id}/next"))
                .await?;
            if resp["data"].is_null() {
                println!("(нет больше pending-уроков)");
                return Ok(());
            }
            let l = &resp["data"];
            println!(
                "доставлен урок #{}: {}",
                l["lesson_num"], l["title"]
            );
            println!();
            println!("{}", l["content"].as_str().unwrap_or(""));
        }
        LearnCmd::Learned { lesson_id } => {
            cli.post_empty(&format!("/api/learning/lessons/{lesson_id}/learned"))
                .await?;
            println!("помечен как изучен: {lesson_id}");
        }
        LearnCmd::Skip { lesson_id } => {
            cli.post_empty(&format!("/api/learning/lessons/{lesson_id}/skip"))
                .await?;
            println!("пропущен: {lesson_id}");
        }
        LearnCmd::Rm { track_id } => {
            cli.delete(&format!("/api/learning/tracks/{track_id}"))
                .await?;
            println!("удалён трек: {track_id}");
        }
    }
    Ok(())
}

// ------------------------ graph command ------------------------

async fn graph_cmd(cli: &Client, id: &str, depth: u8) -> Result<()> {
    use colored::Colorize;

    let resp = cli
        .get(&format!("/api/graph/{id}?depth={depth}"))
        .await?;
    let root_id = resp["root"].as_str().unwrap_or("");
    let nodes_arr = resp["nodes"].as_array().cloned().unwrap_or_default();
    let edges_arr = resp["edges"].as_array().cloned().unwrap_or_default();

    // build maps
    let mut title_by_id: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for n in &nodes_arr {
        let nid = n["id"].as_str().unwrap_or("").to_string();
        let t = n["title"].as_str().unwrap_or("").to_string();
        title_by_id.insert(nid, t);
    }

    let root_title = title_by_id
        .get(root_id)
        .cloned()
        .unwrap_or_else(|| "(unknown)".to_string());
    println!(
        "{}  {}  {}",
        root_title.bold(),
        format!("[{}]", &root_id[..8.min(root_id.len())]).dimmed(),
        "(root)".dimmed()
    );

    // Collect edges from root
    let root_edges: Vec<_> = edges_arr
        .iter()
        .filter(|e| e["from"].as_str() == Some(root_id))
        .collect();

    let n = root_edges.len();
    for (i, e) in root_edges.iter().enumerate() {
        let last = i == n - 1;
        let branch = if last { "└─" } else { "├─" };
        let to = e["to"].as_str().unwrap_or("");
        let kind = e["kind"].as_str().unwrap_or("");
        let title = title_by_id
            .get(to)
            .cloned()
            .unwrap_or_else(|| "(?)".to_string());

        let (indicator, score_str, color_code) = match kind {
            "semantic" => {
                let score = e["score"].as_f64().unwrap_or(0.0);
                let color = if score >= 0.8 {
                    "green"
                } else if score >= 0.6 {
                    "yellow"
                } else {
                    "white"
                };
                ("●", format!("semantic {score:.2}"), color)
            }
            "wiki" => ("○", "wiki-link".to_string(), "cyan"),
            _ => ("·", kind.to_string(), "white"),
        };

        let indicator_colored = match color_code {
            "green" => indicator.green(),
            "yellow" => indicator.yellow(),
            "cyan" => indicator.cyan(),
            _ => indicator.normal(),
        };

        println!(
            "{} {}  {:<40}  {}  {}",
            branch,
            indicator_colored,
            title,
            format!("[{}]", &to[..8.min(to.len())]).dimmed(),
            score_str.dimmed()
        );
    }

    if root_edges.is_empty() {
        println!("  {}", "(связей не найдено)".dimmed());
    }
    Ok(())
}
