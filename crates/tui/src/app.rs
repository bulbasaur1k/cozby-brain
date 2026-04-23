//! App state + async event bus.
//!
//! UI-loop НИКОГДА не блокируется на сеть: все API-вызовы уходят в
//! background-таск через [`AppCmd`], а результаты приходят обратно как
//! [`AppEvent`] — render-loop просто читает их и обновляет state.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde_json::Value;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Inbox,
    Notes,
    Todos,
    Reminders,
    Learning,
    Docs,
}

impl Tab {
    pub const ALL: [Tab; 6] = [
        Tab::Inbox,
        Tab::Notes,
        Tab::Todos,
        Tab::Reminders,
        Tab::Learning,
        Tab::Docs,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Tab::Inbox => "Inbox",
            Tab::Notes => "Notes",
            Tab::Todos => "Todos",
            Tab::Reminders => "Reminders",
            Tab::Learning => "Learning",
            Tab::Docs => "Docs",
        }
    }

    /// API path for list endpoints (None for Inbox).
    pub fn list_path(&self) -> Option<&'static str> {
        match self {
            Tab::Inbox => None,
            Tab::Notes => Some("/api/notes"),
            Tab::Todos => Some("/api/todos"),
            Tab::Reminders => Some("/api/reminders"),
            Tab::Learning => Some("/api/learning/tracks"),
            Tab::Docs => Some("/api/doc/projects"),
        }
    }
}

// ─── panes / focus ──────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Sidebar,
    List,
    Detail,
}

impl Focus {
    pub fn next(self) -> Self {
        match self {
            Focus::Sidebar => Focus::List,
            Focus::List => Focus::Detail,
            Focus::Detail => Focus::Sidebar,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Focus::Sidebar => Focus::Detail,
            Focus::List => Focus::Sidebar,
            Focus::Detail => Focus::List,
        }
    }
}

// ─── commands & events ───────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Kind {
    Note,
    Todo,
    Reminder,
    DocProject,
    DocPage,
}

#[derive(Debug, Clone)]
pub enum AppCmd {
    Refresh(Tab),
    Ping,
    Ingest(String),
    /// Toggle `done` on a todo (true to complete).
    CompleteTodo(String),
    /// Delete item by kind + id.
    Delete(Kind, String),
    /// Load pages for a given project (for docs tree expansion).
    LoadPages(String),
    /// Load full detail of a note by id.
    LoadNote(String),
    /// Load full detail of a doc page.
    LoadPage(String),
}

#[derive(Debug)]
#[allow(dead_code)]
pub enum AppEvent {
    Loaded(Tab, Vec<Value>),
    PagesLoaded(String, Vec<Value>),
    NoteDetail(Value),
    PageDetail(Value),
    Ingested(Value),
    Deleted(Kind, String),
    Pong(bool),
    Error(String),
    Tick,
}

// ─── modes ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Ingest,
    Search,
    Command, // `:cmd`
    Confirm, // y/n overlay
}

#[derive(Debug, Clone)]
pub enum PendingConfirm {
    Delete(Kind, String, String), // kind, id, label-for-display
}

// ─── doc tree row ────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum DocRow {
    Project {
        value: Value,
        expanded: bool,
        page_count: usize,
    },
    Page {
        value: Value,
        #[allow(dead_code)]
        project_slug: String,
    },
}

#[allow(dead_code)]
impl DocRow {
    pub fn id(&self) -> String {
        match self {
            DocRow::Project { value, .. } | DocRow::Page { value, .. } => {
                value["id"].as_str().unwrap_or("").to_string()
            }
        }
    }
}

// ─── todo filter ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoFilter {
    All,
    /// Show only recent: undone (due within N days OR no due date) + done within last N days.
    LastDays(i64),
}

// ─── App ─────────────────────────────────────────────────────────────

pub struct App {
    pub api: String,
    pub tab: Tab,
    pub mode: Mode,
    pub focus: Focus,

    pub input: String,   // ingest input
    pub search: String,  // search filter
    pub command: String, // :command buffer
    pub status: String,
    pub connected: bool,

    pub items: Vec<Value>,
    pub selected: usize,
    pub loading: bool,
    pub spinner: usize,
    pub last_ingest: Option<Value>,
    pub detail_scroll: u16,

    /// Full-screen detail of opened item (None = list mode).
    pub opened: Option<Value>,

    /// Expanded projects in Docs tab (project_id → set).
    pub expanded_projects: HashSet<String>,
    /// Loaded pages per project (project_id → Vec<page>).
    pub doc_pages: HashMap<String, Vec<Value>>,

    pub todo_filter: TodoFilter,
    pub confirm: Option<PendingConfirm>,

    /// @path-автодополнение: подсказки по текущему токену и выбранный индекс.
    pub completions: Vec<Completion>,
    pub completion_index: usize,

    pub cmd_tx: UnboundedSender<AppCmd>,
}

impl App {
    pub fn new(api: String, cmd_tx: UnboundedSender<AppCmd>) -> Self {
        Self {
            api,
            tab: Tab::Inbox,
            mode: Mode::Normal,
            focus: Focus::List,
            input: String::new(),
            search: String::new(),
            command: String::new(),
            status: String::new(),
            connected: false,
            items: Vec::new(),
            selected: 0,
            loading: false,
            spinner: 0,
            last_ingest: None,
            detail_scroll: 0,
            opened: None,
            expanded_projects: HashSet::new(),
            doc_pages: HashMap::new(),
            todo_filter: TodoFilter::LastDays(5),
            confirm: None,
            completions: Vec::new(),
            completion_index: 0,
            cmd_tx,
        }
    }

    // ─── @path completion ────────────────────────────────────────────

    /// Перечитывает список файлов/каталогов по текущему @-токену в input.
    /// Вызывается после любой правки input в ingest-режиме.
    pub fn recompute_completions(&mut self) {
        self.completions = compute_completions(&self.input);
        // Держим индекс в валидном диапазоне.
        if self.completion_index >= self.completions.len() {
            self.completion_index = 0;
        }
    }

    pub fn completion_next(&mut self) {
        if self.completions.is_empty() {
            return;
        }
        self.completion_index = (self.completion_index + 1) % self.completions.len();
    }

    pub fn completion_prev(&mut self) {
        if self.completions.is_empty() {
            return;
        }
        self.completion_index = if self.completion_index == 0 {
            self.completions.len() - 1
        } else {
            self.completion_index - 1
        };
    }

    /// Заменяет текущий @-токен в input на выбранную подсказку.
    /// Возвращает true если что-то приняли.
    pub fn accept_completion(&mut self) -> bool {
        let Some(comp) = self.completions.get(self.completion_index).cloned() else {
            return false;
        };
        if let Some((start, _)) = at_token_bounds(&self.input) {
            self.input.truncate(start);
            self.input.push_str(&comp.insert);
        } else {
            self.input.push_str(&comp.insert);
        }
        // Если выбрали каталог — держим попап открытым и показываем его
        // содержимое, чтобы можно было продолжить углубляться.
        if comp.is_dir {
            self.recompute_completions();
        } else {
            self.completions.clear();
        }
        true
    }
}

#[derive(Debug, Clone)]
pub struct Completion {
    /// Что подставить в input (вместе с `@` в начале).
    pub insert: String,
    /// Как отобразить в попапе (имя файла/каталога, у каталогов — со слешом).
    pub display: String,
    pub is_dir: bool,
}

/// Возвращает `(byte_offset_начала, @-токен)` если последнее «слово» в input
/// начинается с `@`. Иначе None.
pub fn at_token_bounds(input: &str) -> Option<(usize, &str)> {
    // Найти байтовое начало последнего whitespace-разделённого слова.
    let start = input
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_whitespace())
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    let word = &input[start..];
    if word.starts_with('@') {
        Some((start, word))
    } else {
        None
    }
}

/// По текущему @-токену в input строит список файлов/каталогов.
pub fn compute_completions(input: &str) -> Vec<Completion> {
    let Some((_, token)) = at_token_bounds(input) else {
        return Vec::new();
    };
    // token начинается с '@'. Убираем префикс — получаем "сырую" часть пути.
    let raw = &token[1..];
    let (dir_raw, prefix) = split_dir_prefix(raw);
    let dir_abs = if dir_raw.is_empty() {
        std::path::PathBuf::from(".")
    } else {
        expand_tilde(&dir_raw)
    };

    let entries = match std::fs::read_dir(&dir_abs) {
        Ok(it) => it,
        Err(_) => return Vec::new(),
    };

    let prefix_lc = prefix.to_lowercase();
    let mut out: Vec<Completion> = Vec::new();
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        // Скрываем дотфайлы если пользователь сам не начал с точки.
        if name.starts_with('.') && !prefix.starts_with('.') {
            continue;
        }
        if !name.to_lowercase().starts_with(&prefix_lc) {
            continue;
        }
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let suffix = if is_dir { "/" } else { "" };
        out.push(Completion {
            insert: format!("@{dir_raw}{name}{suffix}"),
            display: format!("{name}{suffix}"),
            is_dir,
        });
    }
    // Каталоги вверх, затем алфавит.
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.display.cmp(&b.display)));
    out.truncate(20);
    out
}

/// Разбирает `<dir>/<prefix>` — если нет `/`, dir пустой, prefix = весь token.
fn split_dir_prefix(token: &str) -> (String, String) {
    // Особый случай — голый `~`: показываем содержимое $HOME.
    if token == "~" {
        return ("~/".into(), String::new());
    }
    match token.rfind('/') {
        Some(i) => (token[..=i].to_string(), token[i + 1..].to_string()),
        None => (String::new(), token.to_string()),
    }
}

impl App {
    pub fn switch_tab(&mut self, tab: Tab) {
        if self.tab == tab && !self.items.is_empty() {
            return;
        }
        self.tab = tab;
        self.selected = 0;
        self.items.clear();
        self.opened = None;
        self.search.clear();
        self.detail_scroll = 0;
        if tab.list_path().is_some() {
            self.loading = true;
            let _ = self.cmd_tx.send(AppCmd::Refresh(tab));
        }
    }

    pub fn next_tab(&mut self) {
        let i = Tab::ALL.iter().position(|t| *t == self.tab).unwrap_or(0);
        self.switch_tab(Tab::ALL[(i + 1) % Tab::ALL.len()]);
    }

    pub fn prev_tab(&mut self) {
        let n = Tab::ALL.len();
        let i = Tab::ALL.iter().position(|t| *t == self.tab).unwrap_or(0);
        self.switch_tab(Tab::ALL[(i + n - 1) % n]);
    }

    pub fn refresh(&mut self) {
        if self.tab.list_path().is_some() {
            self.loading = true;
            let _ = self.cmd_tx.send(AppCmd::Refresh(self.tab));
        }
    }

    /// Items visible in current list (applies filters: search, todo-window, etc.).
    pub fn filtered_items(&self) -> Vec<Value> {
        // Special: Docs tab → flat tree of (project, expanded_pages) rendered elsewhere
        if self.tab == Tab::Docs {
            return self.items.clone();
        }

        let mut out: Vec<Value> = self.items.clone();

        // Todos window
        if self.tab == Tab::Todos {
            if let TodoFilter::LastDays(n) = self.todo_filter {
                let cutoff = Utc::now() - chrono::Duration::days(n);
                out.retain(|v| {
                    let done = v["done"].as_bool().unwrap_or(false);
                    let due = parse_dt(v["due_at"].as_str());
                    let completed = parse_dt(v["completed_at"].as_str());
                    let created = parse_dt(v["created_at"].as_str()).unwrap_or_else(Utc::now);
                    if done {
                        completed.map(|t| t >= cutoff).unwrap_or(created >= cutoff)
                    } else {
                        due.map(|t| t >= cutoff)
                            .unwrap_or_else(|| created >= cutoff)
                    }
                });
            }
        }

        if !self.search.is_empty() {
            let q = self.search.to_lowercase();
            out.retain(|v| {
                let title = v["title"]
                    .as_str()
                    .or_else(|| v["text"].as_str())
                    .unwrap_or("")
                    .to_lowercase();
                let tags = v["tags"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str())
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .unwrap_or_default()
                    .to_lowercase();
                title.contains(&q) || tags.contains(&q)
            });
        }

        out
    }

    /// Docs tab — flat list of rows (projects + expanded pages beneath them).
    pub fn docs_rows(&self) -> Vec<DocRow> {
        let mut rows = Vec::new();
        for proj in &self.items {
            let pid = proj["id"].as_str().unwrap_or("").to_string();
            let slug = proj["slug"].as_str().unwrap_or("").to_string();
            let expanded = self.expanded_projects.contains(&pid);
            let pages = self.doc_pages.get(&pid);
            rows.push(DocRow::Project {
                value: proj.clone(),
                expanded,
                page_count: pages.map(|p| p.len()).unwrap_or(0),
            });
            if expanded {
                if let Some(pages) = pages {
                    for page in pages {
                        rows.push(DocRow::Page {
                            value: page.clone(),
                            project_slug: slug.clone(),
                        });
                    }
                }
            }
        }
        rows
    }

    pub fn select_next(&mut self) {
        let len = self.current_len();
        if len > 0 && self.selected + 1 < len {
            self.selected += 1;
            self.detail_scroll = 0;
        }
    }

    pub fn select_prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            self.detail_scroll = 0;
        }
    }

    pub fn select_first(&mut self) {
        self.selected = 0;
        self.detail_scroll = 0;
    }

    pub fn select_last(&mut self) {
        let len = self.current_len();
        self.selected = len.saturating_sub(1);
        self.detail_scroll = 0;
    }

    fn current_len(&self) -> usize {
        if self.tab == Tab::Docs {
            self.docs_rows().len()
        } else {
            self.filtered_items().len()
        }
    }

    pub fn tick(&mut self) {
        self.spinner = (self.spinner + 1) % SPINNER_FRAMES.len();
    }

    pub fn spinner_frame(&self) -> &'static str {
        SPINNER_FRAMES[self.spinner]
    }

    /// Called on Enter / `o` — open detail for selected item.
    pub fn open_selected(&mut self) {
        if self.tab == Tab::Docs {
            let rows = self.docs_rows();
            if let Some(row) = rows.get(self.selected).cloned() {
                match row {
                    DocRow::Project { value, expanded, .. } => {
                        let pid = value["id"].as_str().unwrap_or("").to_string();
                        if expanded {
                            self.expanded_projects.remove(&pid);
                        } else {
                            self.expanded_projects.insert(pid.clone());
                            // load pages if not cached
                            if !self.doc_pages.contains_key(&pid) {
                                let _ = self.cmd_tx.send(AppCmd::LoadPages(pid));
                            }
                        }
                    }
                    DocRow::Page { value, .. } => {
                        let pid = value["id"].as_str().unwrap_or("").to_string();
                        self.loading = true;
                        let _ = self.cmd_tx.send(AppCmd::LoadPage(pid));
                    }
                }
            }
        } else {
            let items = self.filtered_items();
            if let Some(item) = items.get(self.selected).cloned() {
                // For notes we want to fetch full content (list endpoint may truncate in future)
                if self.tab == Tab::Notes {
                    if let Some(id) = item["id"].as_str() {
                        self.loading = true;
                        let _ = self.cmd_tx.send(AppCmd::LoadNote(id.to_string()));
                        return;
                    }
                }
                self.opened = Some(item);
                self.focus = Focus::Detail;
                self.detail_scroll = 0;
            }
        }
    }

    pub fn close_detail(&mut self) {
        self.opened = None;
        self.detail_scroll = 0;
        self.focus = Focus::List;
    }

    /// Called on Space — toggle todo done.
    pub fn toggle_todo_done(&mut self) {
        if self.tab != Tab::Todos {
            return;
        }
        let items = self.filtered_items();
        if let Some(item) = items.get(self.selected) {
            if let Some(id) = item["id"].as_str() {
                let _ = self.cmd_tx.send(AppCmd::CompleteTodo(id.to_string()));
            }
        }
    }

    /// Called on `d` / `:delete` — start confirmation.
    pub fn request_delete(&mut self) {
        if self.tab == Tab::Docs {
            let rows = self.docs_rows();
            if let Some(row) = rows.get(self.selected) {
                let (kind, id, label) = match row {
                    DocRow::Project { value, .. } => (
                        Kind::DocProject,
                        value["id"].as_str().unwrap_or("").to_string(),
                        format!(
                            "project \"{}\"",
                            value["title"].as_str().unwrap_or("?")
                        ),
                    ),
                    DocRow::Page { value, .. } => (
                        Kind::DocPage,
                        value["id"].as_str().unwrap_or("").to_string(),
                        format!("page \"{}\"", value["title"].as_str().unwrap_or("?")),
                    ),
                };
                self.confirm = Some(PendingConfirm::Delete(kind, id, label));
                self.mode = Mode::Confirm;
            }
            return;
        }
        let items = self.filtered_items();
        if let Some(item) = items.get(self.selected) {
            let id = item["id"].as_str().unwrap_or("").to_string();
            let (kind, label) = match self.tab {
                Tab::Notes => (
                    Kind::Note,
                    format!("note \"{}\"", item["title"].as_str().unwrap_or("?")),
                ),
                Tab::Todos => (
                    Kind::Todo,
                    format!("todo \"{}\"", item["title"].as_str().unwrap_or("?")),
                ),
                Tab::Reminders => (
                    Kind::Reminder,
                    format!("reminder \"{}\"", item["text"].as_str().unwrap_or("?")),
                ),
                _ => return,
            };
            self.confirm = Some(PendingConfirm::Delete(kind, id, label));
            self.mode = Mode::Confirm;
        }
    }

    pub fn confirm_yes(&mut self) {
        if let Some(PendingConfirm::Delete(kind, id, _)) = self.confirm.take() {
            let _ = self.cmd_tx.send(AppCmd::Delete(kind, id));
        }
        self.mode = Mode::Normal;
    }

    pub fn confirm_no(&mut self) {
        self.confirm = None;
        self.mode = Mode::Normal;
    }

    /// Execute a `:command`.
    pub fn run_command(&mut self, cmd: &str) {
        let c = cmd.trim();
        match c {
            "q" | "quit" | "exit" => {
                self.status = "<quit via :q>".into();
                let _ = self.cmd_tx.send(AppCmd::Ping); // no-op but placeholder
            }
            "w" | "r" | "refresh" => self.refresh(),
            "inbox" => self.switch_tab(Tab::Inbox),
            "notes" => self.switch_tab(Tab::Notes),
            "todos" => self.switch_tab(Tab::Todos),
            "reminders" | "remind" => self.switch_tab(Tab::Reminders),
            "learn" | "learning" => self.switch_tab(Tab::Learning),
            "docs" | "doc" => self.switch_tab(Tab::Docs),
            "all" => {
                self.todo_filter = TodoFilter::All;
                self.status = "todo filter: all".into();
            }
            "recent" => {
                self.todo_filter = TodoFilter::LastDays(5);
                self.status = "todo filter: last 5 days".into();
            }
            "d" | "del" | "delete" | "rm" => self.request_delete(),
            "open" | "o" => self.open_selected(),
            "close" => self.close_detail(),
            "ingest" | "i" => {
                self.switch_tab(Tab::Inbox);
                self.mode = Mode::Ingest;
                self.input.clear();
            }
            other => {
                self.status = format!("unknown command: :{other}");
            }
        }
    }
}

fn parse_dt(s: Option<&str>) -> Option<DateTime<Utc>> {
    s.and_then(|x| DateTime::parse_from_rfc3339(x).ok().map(|d| d.with_timezone(&Utc)))
}

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

// ─── worker ──────────────────────────────────────────────────────────

pub async fn spawn_worker(
    api: String,
    mut cmd_rx: UnboundedReceiver<AppCmd>,
    ev_tx: UnboundedSender<AppEvent>,
) {
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .expect("reqwest");

    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            AppCmd::Ping => {
                let url = format!("{api}/health");
                let ok = http
                    .get(&url)
                    .send()
                    .await
                    .map(|r| r.status().is_success())
                    .unwrap_or(false);
                let _ = ev_tx.send(AppEvent::Pong(ok));
            }
            AppCmd::Refresh(tab) => {
                let Some(path) = tab.list_path() else { continue };
                let url = format!("{api}{path}");
                match http.get(&url).send().await {
                    Ok(r) => match r.json::<Value>().await {
                        Ok(v) => {
                            let items = v["data"].as_array().cloned().unwrap_or_default();
                            let _ = ev_tx.send(AppEvent::Loaded(tab, items));
                        }
                        Err(e) => {
                            let _ = ev_tx.send(AppEvent::Error(format!("json: {e}")));
                        }
                    },
                    Err(e) => {
                        let _ = ev_tx.send(AppEvent::Error(format!("get: {e}")));
                    }
                }
            }
            AppCmd::Ingest(raw) => {
                let resolved = match resolve_at_prefix(&raw).await {
                    Ok(text) => text,
                    Err(e) => {
                        let _ = ev_tx.send(AppEvent::Error(format!("@: {e}")));
                        continue;
                    }
                };
                #[derive(serde::Serialize)]
                struct Req<'a> {
                    raw: &'a str,
                }
                let url = format!("{api}/api/ingest");
                match http.post(&url).json(&Req { raw: &resolved }).send().await {
                    Ok(r) => match r.json::<Value>().await {
                        Ok(v) => {
                            let _ = ev_tx.send(AppEvent::Ingested(v));
                        }
                        Err(e) => {
                            let _ = ev_tx.send(AppEvent::Error(format!("ingest json: {e}")));
                        }
                    },
                    Err(e) => {
                        let _ = ev_tx.send(AppEvent::Error(format!("ingest: {e}")));
                    }
                }
            }
            AppCmd::CompleteTodo(id) => {
                let url = format!("{api}/api/todos/{id}/complete");
                let _ = http.post(&url).send().await;
                let _ = ev_tx.send(AppEvent::Loaded(Tab::Todos, Vec::new())); // trigger UI refresh
                // actually: request fresh list
                let list_url = format!("{api}/api/todos");
                if let Ok(r) = http.get(&list_url).send().await {
                    if let Ok(v) = r.json::<Value>().await {
                        let items = v["data"].as_array().cloned().unwrap_or_default();
                        let _ = ev_tx.send(AppEvent::Loaded(Tab::Todos, items));
                    }
                }
            }
            AppCmd::Delete(kind, id) => {
                let path = match kind {
                    Kind::Note => format!("/api/notes/{id}"),
                    Kind::Todo => format!("/api/todos/{id}"),
                    Kind::Reminder => format!("/api/reminders/{id}"),
                    Kind::DocProject => format!("/api/doc/projects/{id}"),
                    Kind::DocPage => format!("/api/doc/pages/{id}"),
                };
                let url = format!("{api}{path}");
                match http.delete(&url).send().await {
                    Ok(r) if r.status().is_success() => {
                        let _ = ev_tx.send(AppEvent::Deleted(kind.clone(), id.clone()));
                        // refresh list
                        let tab = match kind {
                            Kind::Note => Tab::Notes,
                            Kind::Todo => Tab::Todos,
                            Kind::Reminder => Tab::Reminders,
                            Kind::DocProject | Kind::DocPage => Tab::Docs,
                        };
                        if let Some(p) = tab.list_path() {
                            let url = format!("{api}{p}");
                            if let Ok(r) = http.get(&url).send().await {
                                if let Ok(v) = r.json::<Value>().await {
                                    let items = v["data"].as_array().cloned().unwrap_or_default();
                                    let _ = ev_tx.send(AppEvent::Loaded(tab, items));
                                }
                            }
                        }
                    }
                    Ok(r) => {
                        let _ = ev_tx.send(AppEvent::Error(format!("delete: {}", r.status())));
                    }
                    Err(e) => {
                        let _ = ev_tx.send(AppEvent::Error(format!("delete: {e}")));
                    }
                }
            }
            AppCmd::LoadPages(project_id) => {
                let url = format!("{api}/api/doc/projects/{project_id}/pages");
                if let Ok(r) = http.get(&url).send().await {
                    if let Ok(v) = r.json::<Value>().await {
                        let items = v["data"].as_array().cloned().unwrap_or_default();
                        let _ = ev_tx.send(AppEvent::PagesLoaded(project_id, items));
                    }
                }
            }
            AppCmd::LoadNote(id) => {
                let url = format!("{api}/api/notes/{id}");
                if let Ok(r) = http.get(&url).send().await {
                    if let Ok(v) = r.json::<Value>().await {
                        let data = v["data"].clone();
                        let _ = ev_tx.send(AppEvent::NoteDetail(data));
                    }
                }
            }
            AppCmd::LoadPage(id) => {
                let url = format!("{api}/api/doc/pages/{id}");
                if let Ok(r) = http.get(&url).send().await {
                    if let Ok(v) = r.json::<Value>().await {
                        let data = v["data"].clone();
                        let _ = ev_tx.send(AppEvent::PageDetail(data));
                    }
                }
            }
        }
    }
}

// ─── @-prefix commands in ingest input ───────────────────────────────
//
// Currently supports a single form: `@<path> [extra text]` — read the file
// and prepend its contents to the raw text sent to `/api/ingest`. More
// @-commands (@url, @clipboard, …) can slot in here later without changing
// callers, because the resolver only touches the string before it leaves
// the worker.

async fn resolve_at_prefix(input: &str) -> Result<String, String> {
    let s = input.trim_start();
    if !s.starts_with('@') {
        return Ok(input.to_string());
    }
    let rest = &s[1..];
    let (token, extra) = match rest.find(char::is_whitespace) {
        Some(i) => (&rest[..i], rest[i..].trim()),
        None => (rest, ""),
    };
    if token.is_empty() {
        return Err("empty @ token — expected path".into());
    }

    // For now every @token is a file path. Future @commands would branch here.
    let path = expand_tilde(token);
    let content = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| format!("read {}: {e}", path.display()))?;

    if extra.is_empty() {
        Ok(content)
    } else {
        Ok(format!("{content}\n\n---\n\n{extra}"))
    }
}

fn expand_tilde(p: &str) -> std::path::PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return std::path::PathBuf::from(home).join(rest);
        }
    }
    std::path::PathBuf::from(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn plain_text_passes_through() {
        let out = resolve_at_prefix("hello world").await.unwrap();
        assert_eq!(out, "hello world");
    }

    #[tokio::test]
    async fn at_path_reads_file() {
        let dir = std::env::temp_dir().join("cozby_at_test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("note.md");
        std::fs::write(&file, "file body").unwrap();
        let input = format!("@{}", file.display());
        let out = resolve_at_prefix(&input).await.unwrap();
        assert_eq!(out, "file body");
    }

    #[tokio::test]
    async fn at_path_with_extra_text_appends() {
        let dir = std::env::temp_dir().join("cozby_at_extra");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("note.md");
        std::fs::write(&file, "body").unwrap();
        let input = format!("@{}  also tag: rust", file.display());
        let out = resolve_at_prefix(&input).await.unwrap();
        assert!(out.contains("body"));
        assert!(out.contains("also tag: rust"));
    }

    #[tokio::test]
    async fn missing_file_returns_error() {
        let err = resolve_at_prefix("@/definitely/not/a/real/file.md")
            .await
            .unwrap_err();
        assert!(err.starts_with("read "));
    }

    #[tokio::test]
    async fn empty_token_errors() {
        let err = resolve_at_prefix("@").await.unwrap_err();
        assert!(err.contains("empty"));
    }

    // ─── @path autocompletion ──────────────────────────────────────

    #[test]
    fn at_token_bounds_basic() {
        assert_eq!(at_token_bounds("@foo"), Some((0, "@foo")));
        assert_eq!(at_token_bounds("hello @bar"), Some((6, "@bar")));
        assert_eq!(at_token_bounds("hello world"), None);
        assert_eq!(at_token_bounds("@"), Some((0, "@")));
        // Перенос строки тоже считается whitespace
        assert_eq!(at_token_bounds("line1\n@path"), Some((6, "@path")));
        // @ внутри слова — не токен (нет whitespace перед)
        assert_eq!(at_token_bounds("foo@bar"), None);
    }

    #[test]
    fn compute_completions_empty_input_no_results() {
        assert!(compute_completions("").is_empty());
        assert!(compute_completions("plain text").is_empty());
    }

    // Параллельно гоняемые тесты не трогают `std::env::current_dir()` —
    // вместо этого передаём в @-токене абсолютный путь до песочницы.
    #[test]
    fn compute_completions_lists_dir_after_at() {
        let sandbox = std::env::temp_dir().join("cozby_at_complete");
        let _ = std::fs::remove_dir_all(&sandbox);
        std::fs::create_dir_all(&sandbox).unwrap();
        std::fs::write(sandbox.join("readme.md"), "").unwrap();
        std::fs::write(sandbox.join("notes.md"), "").unwrap();
        std::fs::create_dir_all(sandbox.join("src")).unwrap();

        let base = sandbox.display().to_string();
        let out = compute_completions(&format!("@{base}/"));
        let names: Vec<String> = out.iter().map(|c| c.display.clone()).collect();
        assert_eq!(names.first().map(|s| s.as_str()), Some("src/"));
        assert!(names.contains(&"readme.md".into()));
        assert!(names.contains(&"notes.md".into()));
        assert!(out.iter().find(|c| c.display == "src/").unwrap().is_dir);

        let only_r = compute_completions(&format!("@{base}/r"));
        assert_eq!(
            only_r.iter().map(|c| c.display.as_str()).collect::<Vec<_>>(),
            vec!["readme.md"]
        );
        assert_eq!(only_r[0].insert, format!("@{base}/readme.md"));

        let dir_comp = compute_completions(&format!("@{base}/s"));
        assert_eq!(dir_comp[0].display, "src/");
        assert_eq!(dir_comp[0].insert, format!("@{base}/src/"));

        let _ = std::fs::remove_dir_all(&sandbox);
    }

    #[test]
    fn compute_completions_dotfiles_hidden_by_default() {
        let sandbox = std::env::temp_dir().join("cozby_at_dot");
        let _ = std::fs::remove_dir_all(&sandbox);
        std::fs::create_dir_all(&sandbox).unwrap();
        std::fs::write(sandbox.join(".env"), "").unwrap();
        std::fs::write(sandbox.join("visible.txt"), "").unwrap();

        let base = sandbox.display().to_string();
        let plain = compute_completions(&format!("@{base}/"));
        assert!(plain.iter().all(|c| !c.display.starts_with('.')));

        let dotted = compute_completions(&format!("@{base}/."));
        assert!(dotted.iter().any(|c| c.display == ".env"));

        let _ = std::fs::remove_dir_all(&sandbox);
    }
}
