//! Текст-протокол многошагового агента (для слабых LLM без native tool-calling).
//!
//! Агент на каждом шаге выдаёт ОДИН JSON-объект:
//!  - `{"tool_call": {"name": "...", "args": {...}}}` — вызвать инструмент;
//!  - `{"final": {"node": {...}, "members": [...]}}` — финальный план записи.
//!
//! Парсинг идёт через тот же `extract_json`, что переживает reasoning qwen3.

use serde::Deserialize;
use serde_json::Value;

/// Вызов инструмента агентом.
#[derive(Debug, Clone, Deserialize)]
pub struct ToolCall {
    pub name: String,
    #[serde(default)]
    pub args: Value,
}

/// Спецификация узла из финального плана.
#[derive(Debug, Clone, Deserialize)]
pub struct NodeSpec {
    pub kind: String,
    pub title: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub metadata: Value,
}

/// Член узла, который агент хочет создать/привязать. Тег `type` различает вид.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum MemberSpec {
    Note {
        #[serde(default)]
        role: String,
        #[serde(default)]
        label: String,
        title: String,
        #[serde(default)]
        content: String,
        #[serde(default)]
        tags: Vec<String>,
    },
    Doc {
        #[serde(default)]
        role: String,
        #[serde(default)]
        label: String,
        project: String,
        page: String,
        #[serde(default)]
        content: String,
        #[serde(default)]
        tags: Vec<String>,
    },
    Todo {
        #[serde(default)]
        role: String,
        #[serde(default)]
        label: String,
        title: String,
    },
    Reminder {
        #[serde(default)]
        role: String,
        #[serde(default)]
        label: String,
        text: String,
        remind_at: String,
    },
    Link {
        #[serde(default)]
        role: String,
        #[serde(default)]
        label: String,
        url: String,
    },
}

/// Финальный план: что за узел и какие части в него записать.
#[derive(Debug, Clone, Deserialize)]
pub struct NodePlan {
    pub node: NodeSpec,
    #[serde(default)]
    pub members: Vec<MemberSpec>,
}

/// Решение агента на одном шаге.
#[derive(Debug, Clone)]
pub enum AgentDecision {
    ToolCall(ToolCall),
    Final(NodePlan),
    /// Не удалось распарсить решение (LLM выдал мусор/пустоту).
    Unparseable(String),
}

/// Парсит ответ LLM в решение. Сначала ищет `final`, потом `tool_call`.
pub fn parse_decision(llm_text: &str) -> AgentDecision {
    let Some(json) = crate::llm_use_cases::extract_json(llm_text) else {
        return AgentDecision::Unparseable(llm_text.chars().take(200).collect());
    };
    let Ok(value) = serde_json::from_str::<Value>(json) else {
        return AgentDecision::Unparseable(json.chars().take(200).collect());
    };
    if let Some(final_v) = value.get("final") {
        match serde_json::from_value::<NodePlan>(final_v.clone()) {
            Ok(plan) => return AgentDecision::Final(plan),
            Err(e) => return AgentDecision::Unparseable(format!("bad final plan: {e}")),
        }
    }
    if let Some(tc_v) = value.get("tool_call") {
        match serde_json::from_value::<ToolCall>(tc_v.clone()) {
            Ok(tc) => return AgentDecision::ToolCall(tc),
            Err(e) => return AgentDecision::Unparseable(format!("bad tool_call: {e}")),
        }
    }
    AgentDecision::Unparseable(json.chars().take(200).collect())
}

/// Статическая инструкция по формату ответа — вставляется в system-промпт.
pub const PROTOCOL_INSTRUCTIONS: &str = r#"Ты агент-органайзер. На КАЖДОМ шаге отвечай РОВНО одним JSON-объектом, без пояснений вокруг.

Чтобы собрать данные — вызови инструмент:
{"tool_call": {"name": "<имя>", "args": { ... }}}

Когда данных достаточно — выдай финальный план (создаст узел и привяжет части):
{"final": {
  "node": {"kind": "duty|incident|topic|project|release", "title": "...", "tags": ["..."], "summary": "кратко о узле", "metadata": {}},
  "members": [
    {"type": "doc", "role": "runbook", "label": "Deploy runbook", "project": "<slug>", "page": "runbook-deploy", "content": "markdown..."},
    {"type": "link", "role": "dashboard", "label": "Grafana", "url": "https://..."},
    {"type": "note", "role": "", "label": "...", "title": "...", "content": "...", "tags": ["..."]},
    {"type": "todo", "label": "проверить X", "title": "проверить X"},
    {"type": "reminder", "label": "...", "text": "...", "remind_at": "2026-01-01T10:00:00Z"}
  ]
}}

Правила:
- НЕ зацикливайся: после 1-2 сборов данных выдавай {"final": ...}.
- Ссылки (URL) клади как member type="link", отдельную заметку под ссылку не создавай.
- Если внешних инструментов нет или они не нужны — сразу выдай {"final": ...}.
- summary узла — 1-2 строки, по ним узел потом находят."#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tool_call() {
        let text = "тут думает...\n{\"tool_call\": {\"name\": \"search_kb\", \"args\": {\"query\": \"billing\"}}}";
        match parse_decision(text) {
            AgentDecision::ToolCall(tc) => {
                assert_eq!(tc.name, "search_kb");
                assert_eq!(tc.args["query"], "billing");
            }
            other => panic!("expected tool_call, got {other:?}"),
        }
    }

    #[test]
    fn parses_final_plan_with_members() {
        let text = r#"{"final": {"node": {"kind": "duty", "title": "Дежурство billing", "tags": ["oncall"], "summary": "s"}, "members": [{"type": "link", "role": "dashboard", "label": "Grafana", "url": "https://g/x"}, {"type": "doc", "project": "billing", "page": "runbook-deploy", "content": "c", "role": "runbook", "label": "Deploy"}]}}"#;
        match parse_decision(text) {
            AgentDecision::Final(plan) => {
                assert_eq!(plan.node.kind, "duty");
                assert_eq!(plan.members.len(), 2);
                assert!(matches!(plan.members[0], MemberSpec::Link { .. }));
                assert!(matches!(plan.members[1], MemberSpec::Doc { .. }));
            }
            other => panic!("expected final, got {other:?}"),
        }
    }

    #[test]
    fn unparseable_on_garbage() {
        assert!(matches!(parse_decision("no json here"), AgentDecision::Unparseable(_)));
    }
}
