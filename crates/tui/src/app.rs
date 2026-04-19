//! App state + async event bus.
//!
//! UI-loop НИКОГДА не блокируется на сеть: все API-вызовы уходят в
//! background-таск через [`AppCmd`], а результаты приходят обратно как
//! [`AppEvent`] — render-loop просто читает их и обновляет state.

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

// ─── commands & events ───────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum AppCmd {
    /// Load list for given tab.
    Refresh(Tab),
    /// Check server health.
    Ping,
    /// Send raw text to /api/ingest.
    Ingest(String),
}

#[derive(Debug)]
#[allow(dead_code)]
pub enum AppEvent {
    Loaded(Tab, Vec<Value>),
    Ingested(Value),
    Pong(bool),
    Error(String),
    /// Spinner tick (reserved for future external tickers).
    Tick,
}

// ─── running state ───────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Navigate tabs/items.
    Normal,
    /// Typing into ingest field.
    Ingest,
    /// Typing into search filter.
    Search,
}

pub struct App {
    pub api: String,
    pub tab: Tab,
    pub mode: Mode,

    pub input: String,        // ingest input
    pub search: String,       // current search filter
    pub status: String,       // bottom-bar status message
    pub connected: bool,

    pub items: Vec<Value>,    // current tab's items
    pub selected: usize,
    pub loading: bool,
    pub spinner: usize,       // tick-based spinner frame
    pub last_ingest: Option<Value>,

    pub cmd_tx: UnboundedSender<AppCmd>,
}

impl App {
    pub fn new(api: String, cmd_tx: UnboundedSender<AppCmd>) -> Self {
        Self {
            api,
            tab: Tab::Inbox,
            mode: Mode::Normal,
            input: String::new(),
            search: String::new(),
            status: String::new(),
            connected: false,
            items: Vec::new(),
            selected: 0,
            loading: false,
            spinner: 0,
            last_ingest: None,
            cmd_tx,
        }
    }

    pub fn switch_tab(&mut self, tab: Tab) {
        if self.tab == tab {
            return;
        }
        self.tab = tab;
        self.selected = 0;
        self.items.clear();
        self.search.clear();
        if let Some(_path) = tab.list_path() {
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

    /// Filtered items according to current `search` string.
    pub fn filtered_items(&self) -> Vec<&Value> {
        if self.search.is_empty() {
            return self.items.iter().collect();
        }
        let q = self.search.to_lowercase();
        self.items
            .iter()
            .filter(|v| {
                let title = v["title"].as_str().or_else(|| v["text"].as_str()).unwrap_or("").to_lowercase();
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
            })
            .collect()
    }

    pub fn select_next(&mut self) {
        let len = self.filtered_items().len();
        if len > 0 && self.selected + 1 < len {
            self.selected += 1;
        }
    }

    pub fn select_prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn select_first(&mut self) {
        self.selected = 0;
    }

    pub fn select_last(&mut self) {
        let len = self.filtered_items().len();
        self.selected = len.saturating_sub(1);
    }

    /// Advance spinner frame.
    pub fn tick(&mut self) {
        self.spinner = (self.spinner + 1) % SPINNER_FRAMES.len();
    }

    pub fn spinner_frame(&self) -> &'static str {
        SPINNER_FRAMES[self.spinner]
    }
}

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

// ─── worker: runs API calls, emits events ───────────────────────────

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
                let ok = http.get(&url).send().await.map(|r| r.status().is_success()).unwrap_or(false);
                let _ = ev_tx.send(AppEvent::Pong(ok));
            }
            AppCmd::Refresh(tab) => {
                let path = match tab.list_path() {
                    Some(p) => p,
                    None => continue,
                };
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
        }
    }
}
