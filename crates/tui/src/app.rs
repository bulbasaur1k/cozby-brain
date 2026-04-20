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
            cmd_tx,
        }
    }

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
                #[derive(serde::Serialize)]
                struct Req<'a> {
                    raw: &'a str,
                }
                let url = format!("{api}/api/ingest");
                match http.post(&url).json(&Req { raw: &raw }).send().await {
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
