//! Многошаговый ingest-агент: classify → собрать контекст (RAG + MCP) →
//! составить план → записать в несколько мест, связав в УЗЕЛ (Node).
//!
//! Живёт в `web` (не в `application`), т.к. оркестрирует акторы через `call!`.
//! Чистые типы/парсер — в `application::agent_protocol`. Infallible: при любой
//! проблеме деградирует на старый одношаговый классификатор.

use ractor::call;
use serde_json::Value;

use crate::app_state::AppState;
use crate::handlers::{gather_context_items, index_async};
use actors::doc_actor::{DocMsg, DocOp};
use actors::node_actor::NodeMsg;
use actors::note_actor::NoteMsg;
use actors::reminder_actor::ReminderMsg;
use actors::todo_actor::TodoMsg;
use application::agent_protocol::{
    parse_decision, AgentDecision, MemberSpec, NodePlan, PROTOCOL_INSTRUCTIONS,
};
use application::ports::{KIND_DOC_PAGE, KIND_NODE, KIND_NOTE};
use application::skills::select_skills;
use application::summary_compression::{compress_summary, SummaryCompressionBudget};
use domain::entities::{Node, NodeMember};

pub struct IngestAgent<'a> {
    state: &'a AppState,
    max_iters: usize,
}

pub struct AgentOutcome {
    pub node: Node,
    pub members: Vec<NodeMember>,
    pub iterations: usize,
    pub used_fallback: bool,
    pub log: Vec<String>,
}

impl<'a> IngestAgent<'a> {
    pub fn new(state: &'a AppState) -> Self {
        Self {
            state,
            max_iters: 5,
        }
    }

    /// Запускает агентный цикл. Infallible — всегда возвращает Outcome либо None
    /// (None = совсем не удалось ничего создать, вызывающий откатится на старый путь).
    pub async fn run(&self, raw: &str) -> Option<AgentOutcome> {
        self.state.activity.emit(
            "start",
            "agent",
            format!("агент ingest: {}", raw.chars().take(50).collect::<String>()),
        );
        let system = self.build_system_prompt(raw).await;
        let mut transcript = format!("Запрос пользователя:\n{raw}");
        let mut log: Vec<String> = Vec::new();
        let mut unparseable_strikes = 0;

        for iter in 0..self.max_iters {
            let text = match self.state.llm.complete_text(&system, &transcript).await {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!(error = %e, "agent: llm failed, fallback");
                    return self.fallback(raw, &mut log).await;
                }
            };
            match parse_decision(&text) {
                AgentDecision::Final(plan) => {
                    return self.execute_plan(plan, iter + 1, log).await;
                }
                AgentDecision::ToolCall(tc) => {
                    self.state
                        .activity
                        .emit("info", "agent", format!("шаг {}: инструмент {}", iter + 1, tc.name));
                    let result = self.execute_tool(&tc.name, &tc.args).await;
                    let compact = compress_summary(&result, SummaryCompressionBudget::tool_result());
                    log.push(format!("tool {} → {} chars", tc.name, compact.summary.len()));
                    transcript.push_str(&format!(
                        "\n\n[результат инструмента {}]:\n{}\n\nЕсли данных достаточно — выдай {{\"final\": ...}}.",
                        tc.name, compact.summary
                    ));
                }
                AgentDecision::Unparseable(s) => {
                    unparseable_strikes += 1;
                    log.push(format!("unparseable: {s}"));
                    if unparseable_strikes >= 2 {
                        tracing::warn!("agent: repeated unparseable, fallback");
                        return self.fallback(raw, &mut log).await;
                    }
                    transcript.push_str(
                        "\n\nОтвет должен быть РОВНО одним JSON-объектом по формату. Повтори.",
                    );
                }
            }
        }
        tracing::info!("agent: max_iters reached, fallback");
        self.fallback(raw, &mut log).await
    }

    async fn build_system_prompt(&self, raw: &str) -> String {
        let mut sections: Vec<String> = Vec::new();
        sections.push(
            "Ты — персональный AI-органайзер дежурного/разработчика. Понимаешь, что один \
ввод может быть сразу несколькими сущностями (дока + запись + напоминание), и связываешь \
их в единый УЗЕЛ."
                .to_string(),
        );
        sections.push(PROTOCOL_INSTRUCTIONS.to_string());

        // tool catalog
        let mut tools = vec![
            "- search_kb {\"query\": str} — поиск по личной базе знаний (заметки, доки, узлы)"
                .to_string(),
        ];
        for (full_name, tool) in self.state.mcp.list_all_tools().await {
            let desc = tool.description.unwrap_or_default();
            tools.push(format!("- {full_name} {{...}} — {desc}"));
        }
        sections.push(format!("Доступные инструменты:\n{}", tools.join("\n")));

        // relevant skills
        let selected = select_skills(&self.state.skills, raw);
        if !selected.is_empty() {
            let skills_text = selected
                .iter()
                .map(|s| format!("## Скил: {}\n{}", s.name, s.body))
                .collect::<Vec<_>>()
                .join("\n\n");
            let compact = compress_summary(&skills_text, SummaryCompressionBudget::default());
            sections.push(format!("Применимые инструкции (скилы):\n{}", compact.summary));
        }

        // existing context (RAG, incl. nodes)
        let ctx = gather_context_items(self.state, raw).await;
        if !ctx.is_empty() {
            let ctx_text = ctx
                .iter()
                .map(|(kind, title, tags, preview)| {
                    format!("- [{kind}] {title} (теги: {tags}) — {preview}")
                })
                .collect::<Vec<_>>()
                .join("\n");
            let compact = compress_summary(&ctx_text, SummaryCompressionBudget::default());
            sections.push(format!(
                "Похожие существующие записи (учитывай, не дубли):\n{}",
                compact.summary
            ));
        }

        sections.join("\n\n")
    }

    async fn execute_tool(&self, name: &str, args: &Value) -> String {
        if name == "search_kb" {
            let query = args
                .get("query")
                .and_then(|q| q.as_str())
                .unwrap_or_default();
            return self.search_kb(query).await;
        }
        if name.starts_with("mcp__") {
            let arguments = if args.is_null() { None } else { Some(args.clone()) };
            self.state.activity.emit("start", "mcp", format!("MCP вызов: {name}"));
            return match self.state.mcp.call_tool(name, arguments).await {
                Ok(result) => {
                    self.state.activity.emit("done", "mcp", format!("MCP ответ: {name}"));
                    result
                        .content
                        .iter()
                        .filter_map(|c| match c {
                            mcp::ToolContent::Text { text } => Some(text.clone()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                }
                Err(e) => {
                    self.state.activity.emit("error", "mcp", format!("MCP ошибка {name}: {e}"));
                    format!("ошибка инструмента {name}: {e}")
                }
            };
        }
        format!("неизвестный инструмент: {name}")
    }

    async fn search_kb(&self, query: &str) -> String {
        let items = gather_context_items(self.state, query).await;
        if items.is_empty() {
            return "ничего не найдено".to_string();
        }
        items
            .iter()
            .map(|(kind, title, tags, preview)| format!("[{kind}] {title} ({tags}): {preview}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    async fn execute_plan(
        &self,
        plan: NodePlan,
        iterations: usize,
        mut log: Vec<String>,
    ) -> Option<AgentOutcome> {
        // 1. create node
        let node = match call!(
            self.state.node_actor,
            NodeMsg::Create,
            plan.node.kind.clone(),
            plan.node.title.clone(),
            plan.node.tags.clone()
        ) {
            Ok(Ok(n)) => n,
            Ok(Err(e)) => {
                log.push(format!("node create failed: {e}"));
                tracing::warn!(error = %e, "agent: node create failed");
                return None;
            }
            Err(e) => {
                log.push(format!("node actor error: {e}"));
                return None;
            }
        };

        // 2. summary + metadata
        let mut node = node;
        if !plan.node.summary.is_empty() {
            if let Ok(Ok(n)) = call!(
                self.state.node_actor,
                NodeMsg::UpdateSummary,
                node.id.clone(),
                plan.node.summary.clone()
            ) {
                node = n;
            }
        }
        if plan.node.metadata.is_object()
            && plan.node.metadata.as_object().map(|m| !m.is_empty()).unwrap_or(false)
        {
            if let Ok(Ok(n)) = call!(
                self.state.node_actor,
                NodeMsg::SetMetadata,
                node.id.clone(),
                plan.node.metadata.clone()
            ) {
                node = n;
            }
        }
        index_async(
            self.state.clone(),
            node.id.clone(),
            KIND_NODE,
            node.title.clone(),
            if node.summary.is_empty() { node.title.clone() } else { node.summary.clone() },
            node.tags.clone(),
        );

        // 3. members — последовательно (частичный успех допустим)
        for member in plan.members {
            self.create_member(&node.id, member, &mut log).await;
        }

        // 4. re-read members for the response
        let members = call!(self.state.node_actor, NodeMsg::ListMembers, node.id.clone())
            .unwrap_or_default();

        self.state.activity.emit(
            "done",
            "agent",
            format!("узел создан: {} (+{} членов)", node.title, members.len()),
        );
        Some(AgentOutcome {
            node,
            members,
            iterations,
            used_fallback: false,
            log,
        })
    }

    async fn create_member(&self, node_id: &str, member: MemberSpec, log: &mut Vec<String>) {
        match member {
            MemberSpec::Link { role, label, url } => {
                self.add_member(node_id, "link", &url, &role, &label, log).await;
            }
            MemberSpec::Note {
                role,
                label,
                title,
                content,
                tags,
            } => {
                if let Ok(Ok(n)) =
                    call!(self.state.note_actor, NoteMsg::Create, title, content, tags)
                {
                    index_async(
                        self.state.clone(),
                        n.id.clone(),
                        KIND_NOTE,
                        n.title.clone(),
                        n.content.clone(),
                        n.tags.clone(),
                    );
                    self.add_member(node_id, "note", &n.id, &role, &label, log).await;
                }
            }
            MemberSpec::Doc {
                role,
                label,
                project,
                page,
                content,
                tags,
            } => {
                match call!(
                    self.state.doc_actor,
                    DocMsg::IngestDoc,
                    project,
                    page,
                    content,
                    tags,
                    DocOp::Create,
                    None::<String>,
                    "llm".to_string()
                ) {
                    Ok(Ok(p)) => {
                        index_async(
                            self.state.clone(),
                            p.id.clone(),
                            KIND_DOC_PAGE,
                            p.title.clone(),
                            p.content.clone(),
                            p.tags.clone(),
                        );
                        self.add_member(node_id, "doc_page", &p.id, &role, &label, log).await;
                    }
                    other => log.push(format!("doc create skipped: {other:?}")),
                }
            }
            MemberSpec::Todo { role, label, title } => {
                if let Ok(Ok(t)) =
                    call!(self.state.todo_actor, TodoMsg::Create, title, None::<chrono::DateTime<chrono::Utc>>)
                {
                    self.add_member(node_id, "todo", &t.id, &role, &label, log).await;
                }
            }
            MemberSpec::Reminder {
                role,
                label,
                text,
                remind_at,
            } => {
                match chrono::DateTime::parse_from_rfc3339(&remind_at) {
                    Ok(dt) => {
                        let when = dt.with_timezone(&chrono::Utc);
                        if let Ok(Ok(r)) = call!(
                            self.state.reminder_actor,
                            ReminderMsg::Create,
                            text,
                            when,
                            None::<String>
                        ) {
                            self.add_member(node_id, "reminder", &r.id, &role, &label, log).await;
                        }
                    }
                    Err(e) => log.push(format!("reminder bad date '{remind_at}': {e}")),
                }
            }
        }
    }

    async fn add_member(
        &self,
        node_id: &str,
        kind: &str,
        member_id: &str,
        role: &str,
        label: &str,
        log: &mut Vec<String>,
    ) {
        match call!(
            self.state.node_actor,
            NodeMsg::AddMember,
            node_id.to_string(),
            kind.to_string(),
            member_id.to_string(),
            role.to_string(),
            label.to_string()
        ) {
            Ok(Ok(_)) => log.push(format!("+ {kind} member")),
            Ok(Err(e)) => log.push(format!("member {kind} failed: {e}")),
            Err(e) => log.push(format!("member {kind} actor error: {e}")),
        }
    }

    /// Деградация: старый одношаговый классификатор, обёрнутый в один узел kind=topic.
    async fn fallback(&self, raw: &str, log: &mut Vec<String>) -> Option<AgentOutcome> {
        self.state
            .activity
            .emit("info", "agent", "fallback на классификатор");
        log.push("fallback: classify_and_structure".to_string());
        let now = chrono::Utc::now();
        let ctx = gather_context_items(self.state, raw).await;
        let classified = application::llm_use_cases::classify_and_structure_with_context(
            self.state.llm.as_ref(),
            raw,
            now,
            &ctx,
        )
        .await
        .unwrap_or_default();
        if classified.is_empty() {
            return None;
        }

        // create umbrella node
        let title = raw.lines().next().unwrap_or("Запись").chars().take(80).collect::<String>();
        let node = match call!(
            self.state.node_actor,
            NodeMsg::Create,
            "topic".to_string(),
            title,
            Vec::<String>::new()
        ) {
            Ok(Ok(n)) => n,
            _ => return None,
        };
        index_async(
            self.state.clone(),
            node.id.clone(),
            KIND_NODE,
            node.title.clone(),
            node.title.clone(),
            node.tags.clone(),
        );

        use application::llm_use_cases::Classified;
        for item in classified {
            match item {
                Classified::Note(n) => {
                    if let Ok(Ok(note)) = call!(
                        self.state.note_actor,
                        NoteMsg::Create,
                        n.title,
                        n.content,
                        n.tags
                    ) {
                        index_async(
                            self.state.clone(),
                            note.id.clone(),
                            KIND_NOTE,
                            note.title.clone(),
                            note.content.clone(),
                            note.tags.clone(),
                        );
                        self.add_member(&node.id, "note", &note.id, "", "", log).await;
                    }
                }
                Classified::Todo(t) => {
                    if let Ok(Ok(todo)) =
                        call!(self.state.todo_actor, TodoMsg::Create, t.title, t.due_at)
                    {
                        self.add_member(&node.id, "todo", &todo.id, "", "", log).await;
                    }
                }
                Classified::Reminder(r) => {
                    if let Ok(Ok(rem)) = call!(
                        self.state.reminder_actor,
                        ReminderMsg::Create,
                        r.text,
                        r.remind_at,
                        r.recurrence.clone()
                    ) {
                        self.add_member(&node.id, "reminder", &rem.id, "", "", log).await;
                    }
                }
                _ => {} // Question/Doc в fallback не материализуем как members
            }
        }

        let members = call!(self.state.node_actor, NodeMsg::ListMembers, node.id.clone())
            .unwrap_or_default();
        Some(AgentOutcome {
            node,
            members,
            iterations: 0,
            used_fallback: true,
            log: std::mem::take(log),
        })
    }
}
