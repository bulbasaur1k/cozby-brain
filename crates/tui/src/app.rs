//! App state for cozby-tui.

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Inbox,
    Notes,
    Todos,
    Reminders,
    Learning,
}

impl Tab {
    pub const ALL: [Tab; 5] = [
        Tab::Inbox,
        Tab::Notes,
        Tab::Todos,
        Tab::Reminders,
        Tab::Learning,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Tab::Inbox => "Inbox",
            Tab::Notes => "Notes",
            Tab::Todos => "Todos",
            Tab::Reminders => "Reminders",
            Tab::Learning => "Learning",
        }
    }
}

pub struct App {
    pub api: String,
    pub http: reqwest::Client,
    pub tab: Tab,
    pub input_mode: bool,
    pub input: String,
    pub status: String,
    /// Last /api/ingest response (for Inbox preview).
    pub last_ingest: Option<Value>,
    pub selected: usize,
    pub items: Vec<Value>,
}

impl App {
    pub fn new(api: String) -> Self {
        Self {
            api: api.trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
            tab: Tab::Inbox,
            input_mode: false,
            input: String::new(),
            status: String::new(),
            last_ingest: None,
            selected: 0,
            items: Vec::new(),
        }
    }

    pub fn next_tab(&mut self) {
        let idx = Tab::ALL.iter().position(|t| *t == self.tab).unwrap_or(0);
        self.tab = Tab::ALL[(idx + 1) % Tab::ALL.len()];
        self.selected = 0;
    }

    pub fn prev_tab(&mut self) {
        let idx = Tab::ALL.iter().position(|t| *t == self.tab).unwrap_or(0);
        let n = Tab::ALL.len();
        self.tab = Tab::ALL[(idx + n - 1) % n];
        self.selected = 0;
    }

    pub fn next_item(&mut self) {
        if !self.items.is_empty() && self.selected + 1 < self.items.len() {
            self.selected += 1;
        }
    }

    pub fn prev_item(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub async fn refresh_current_tab(&mut self) {
        let path = match self.tab {
            Tab::Inbox => {
                self.items.clear();
                return;
            }
            Tab::Notes => "/api/notes",
            Tab::Todos => "/api/todos",
            Tab::Reminders => "/api/reminders",
            Tab::Learning => "/api/learning/tracks",
        };
        match self.get(path).await {
            Ok(v) => {
                self.items = v["data"].as_array().cloned().unwrap_or_default();
                self.selected = 0;
                self.status = format!("загружено: {}", self.items.len());
            }
            Err(e) => {
                self.status = format!("ошибка: {e}");
            }
        }
    }

    pub async fn send_ingest(&mut self, raw: &str) {
        #[derive(serde::Serialize)]
        struct Req<'a> {
            raw: &'a str,
        }
        match self.post("/api/ingest", &Req { raw }).await {
            Ok(v) => {
                let kind = v["type"].as_str().unwrap_or("?");
                self.status = format!("LLM classified as: {kind}");

                // Auto-confirm notes with "create" action (no suggestion) for simplicity
                if kind == "note" {
                    let structured = v["structured"].clone();
                    let suggestion = &v["suggestion"];
                    if suggestion.is_null() {
                        // Auto-create without suggestion
                        let title = structured["title"].as_str().unwrap_or("untitled");
                        let content = structured["content"].as_str().unwrap_or("");
                        let tags: Vec<String> = structured["tags"]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(|v| v.as_str())
                                    .map(|s| s.to_string())
                                    .collect()
                            })
                            .unwrap_or_default();
                        #[derive(serde::Serialize)]
                        struct Confirm<'a> {
                            action: &'a str,
                            target_id: Option<String>,
                            title: &'a str,
                            content: &'a str,
                            tags: Vec<String>,
                        }
                        if let Err(e) = self
                            .post(
                                "/api/ingest/note/confirm",
                                &Confirm {
                                    action: "create",
                                    target_id: None,
                                    title,
                                    content,
                                    tags,
                                },
                            )
                            .await
                        {
                            self.status = format!("ошибка confirm: {e}");
                        } else {
                            self.status = format!("создана заметка: {title}");
                        }
                    } else {
                        let s_title = suggestion["target_title"].as_str().unwrap_or("?");
                        self.status =
                            format!("похожая заметка: {s_title}. Нажми 'a' чтобы дополнить, 'n' — создать новую.");
                        // For simplicity: just store the suggestion in last_ingest;
                        // user can use CLI for append decision.
                    }
                }

                self.last_ingest = Some(v);
            }
            Err(e) => {
                self.status = format!("ошибка: {e}");
            }
        }
    }

    async fn get(&self, path: &str) -> anyhow::Result<Value> {
        let url = format!("{}{}", self.api, path);
        let resp = self.http.get(&url).send().await?;
        let status = resp.status();
        let body: Value = resp.json().await?;
        if !status.is_success() {
            anyhow::bail!("[{status}] {}", body["error"].as_str().unwrap_or(""));
        }
        Ok(body)
    }

    async fn post<T: serde::Serialize>(&self, path: &str, body: &T) -> anyhow::Result<Value> {
        let url = format!("{}{}", self.api, path);
        let resp = self.http.post(&url).json(body).send().await?;
        let status = resp.status();
        let v: Value = resp.json().await?;
        if !status.is_success() {
            anyhow::bail!("[{status}] {}", v["error"].as_str().unwrap_or(""));
        }
        Ok(v)
    }
}
