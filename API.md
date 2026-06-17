# cozby-brain — HTTP API контракт

Личный AI-органайзер. REST + один SSE-поток. Только localhost (без auth/rate-limit).

- **Проект (путь на машине):** `/Users/daniilkozin/my/cozby-brain`
- **Репозиторий:** https://github.com/bulbasaur1k/cozby-brain
- **Base URL:** `http://localhost:8081` (env `HTTP_ADDR`, по умолчанию `0.0.0.0:8081`)
- **Запуск:** `./run.sh` из корня проекта (поднимает Postgres+Qdrant+MinIO в docker и сервер на хосте)
- **Content-Type:** `application/json` для тел запросов и ответов (кроме `/api/events` → `text/event-stream`, `/api/ical/feed.ics` → `text/calendar`)
- **CORS:** permissive (любой origin)

## Соглашения по ответам

```jsonc
// успех
{ "status": "ok", "data": <T> }
// ошибка
{ "error": "описание" }
```

Коды: `200` OK · `201` Created · `400` Bad Request · `404` Not Found · `405` Method Not Allowed · `500` · `502`/`503` (LLM недоступен/не настроен).

Некоторые эндпоинты (`/health`, `/api/ingest`, `/api/ask`, `/api/status`) имеют свою форму — см. ниже.

---

## Служебные

### `GET /health`
`200 → { "status": "ok" }`

### `GET /api/status`
Снапшот состояния сервисов (для интеграций/мониторинга).
```jsonc
{ "status": "ok", "data": {
  "llm":       { "name": "openai-compat", "model": "qwen/qwen3-coder-30b-a3b-instruct", "configured": true },
  "embedding": { "configured": true },
  "vector":    { "configured": true },
  "storage":   { "configured": true },
  "mcp": {
    "servers": [ { "name": "dpWiki", "connected": true, "tools": 12 } ],
    "tool_count": 12,
    "agent_enabled": true        // включён ли многошаговый агент (env INGEST_AGENT)
  }
}}
```

### `GET /api/events`  (SSE)
Живой поток активности сервера. `Content-Type: text/event-stream`, каждое событие:
```
data: {"ts":"2026-06-17T07:19:52.298+00:00","level":"start","kind":"llm","detail":"LLM запрос: ..."}
```
- `level`: `start` | `done` | `error` | `info`
- `kind`: `llm` | `embed` | `mcp` | `agent` | `system`

Долгоживущее соединение, keep-alive. Читать инкрементально (chunked).

---

## Universal ingest (LLM)

### `POST /api/ingest`
Главная точка входа. Сырой текст → LLM сам решает, что это.
```jsonc
// запрос
{ "raw": "завтра в 10 созвон с Ваней" }
```

**Поведение зависит от env `INGEST_AGENT`:**

- **`INGEST_AGENT` выкл (по умолчанию)** — одношаговый классификатор. Ответ:
  - один элемент → плоская форма: `{ "type": "todo|reminder|doc|question|note", "status": "ok", "data": {...} }`
  - несколько → `{ "status": "ok", "items": [ {…}, {…} ] }`
  - для `note` действует 2-step (см. ниже): возвращает `{ "type":"note", "structured":{...}, "suggestion": {...}|null }` и НЕ создаёт сразу.
- **`INGEST_AGENT=1`** — многошаговый агент: собирает **узел (node)** из нескольких сущностей (+ MCP-контекст). Ответ:
```jsonc
{ "status": "ok", "type": "node",
  "data": { "node": <Node>, "members": [ <NodeMember>… ] },
  "agent": { "iterations": 2, "used_fallback": false, "log": ["tool search_kb → …","+ doc_page member", …] }
}
```

### `POST /api/ingest/note/confirm`
2-step подтверждение для заметок (после `/api/ingest`, когда вернулся `type:note`).
```jsonc
{
  "action": "create" | "append",
  "target_id": "uuid",     // обязательно для append
  "title": "…",
  "content": "…",          // optional
  "tags": ["…"]            // optional
}
```
`201 → { "status":"ok", "data": <Note> }`

### `GET /api/ask?q=<вопрос>`
RAG Q&A: embedding вопроса → top-6 → ответ с цитатами `[N]`.
```jsonc
{ "status":"ok", "question":"…", "answer":"текст с [1][2]", "sources":[ {"kind":"note","id":"…","title":"…","score":0.82} ] }
```

---

## Nodes — составные сущности («узлы»)

Узел связывает несколько записей (note/todo/reminder/doc_page/link) в один объект.
`member_kind="link"` хранит URL прямо в `member_id` (без отдельной сущности).

### `GET /api/nodes?kind=<duty|incident|topic|project|release>`
`kind` опционален. `200 → { "status":"ok", "data": [ <Node>… ] }`

### `POST /api/nodes`
```jsonc
{ "kind": "duty", "title": "Дежурство billing", "tags": ["oncall"] }
```
`201 → { "status":"ok", "data": <Node> }` · kind вне белого списка → `400`

### `GET /api/nodes/{id}`
`200 → { "status":"ok", "data": { "node": <Node>, "members": [ <NodeMember>… ] } }` · нет → `404`

### `PUT /api/nodes/{id}`
Частичное обновление (любое подмножество полей).
```jsonc
{ "summary": "…", "status": "active|resolved|archived", "metadata": { "project": "billing" } }
```
`200 → { "status":"ok", "data": <Node> }`

### `DELETE /api/nodes/{id}`
Каскадно удаляет members. `200 → { "status":"ok" }`

### `POST /api/nodes/{id}/members`
```jsonc
{ "kind": "link|note|todo|reminder|doc_page",
  "member_id": "https://grafana/x",   // для link — URL; иначе id сущности
  "role": "dashboard",                 // optional: runbook|dashboard|contact|action|…
  "label": "Grafana billing" }         // optional
```
`201 → { "status":"ok", "data": <NodeMember> }`

### `DELETE /api/nodes/{id}/members/{member_id}`
`member_id` = PK строки члена (поле `id` из NodeMember). `200 → { "status":"ok" }`

---

## Notes

| Метод | Путь | Тело / запрос | Ответ |
|---|---|---|---|
| GET | `/api/notes` | — | `data: [Note]` |
| POST | `/api/notes` | `{ "title", "content"?, "tags"? }` | `201 data: Note` |
| GET | `/api/notes/search?q=<str>` | — | `data: [Note]` |
| GET | `/api/notes/{id}` | — | `data: Note, links: ["[[wiki]]"…]` |
| PUT | `/api/notes/{id}` | `{ "title", "content"?, "tags"? }` | `data: Note` |
| DELETE | `/api/notes/{id}` | — | `{ "status":"ok" }` |

## Todos

| Метод | Путь | Тело | Ответ |
|---|---|---|---|
| GET | `/api/todos` | — | `data: [Todo]` |
| POST | `/api/todos` | `{ "title", "due_at"? }` (RFC3339) | `201 data: Todo` |
| POST | `/api/todos/{id}/complete` | — | `data: Todo` |
| DELETE | `/api/todos/{id}` | — | `{ "status":"ok" }` |

## Reminders

| Метод | Путь | Тело | Ответ |
|---|---|---|---|
| GET | `/api/reminders` | — | `data: [Reminder]` |
| POST | `/api/reminders` | `{ "text", "remind_at", "recurrence"? }` | `201 data: Reminder` |
| DELETE | `/api/reminders/{id}` | — | `{ "status":"ok" }` |

`remind_at` — RFC3339 UTC. `recurrence` — RRULE-lite (`FREQ=DAILY`, `FREQ=WEEKLY;BYDAY=MO,WE`, `FREQ=MONTHLY;BYMONTHDAY=15`, опц. `INTERVAL=N`).

### `GET /api/ical/feed.ics`
iCalendar-фид todos+reminders для подписки в Apple/Google/Outlook. `Content-Type: text/calendar`.

---

## Documentation (projects → pages → versions)

| Метод | Путь | Тело / примечание | Ответ |
|---|---|---|---|
| GET | `/api/doc/projects` | — | `data: [Project]` |
| POST | `/api/doc/projects` | `{ "slug"?, "title", "description"?, "tags"? }` | `201 data: Project` |
| GET | `/api/doc/projects/{id}` | id или slug | `data: Project` |
| DELETE | `/api/doc/projects/{id}` | — | `{ "status":"ok" }` |
| GET | `/api/doc/projects/{id}/pages` | — | `data: [DocPage]` |
| POST | `/api/doc/pages` | см. ниже | `data: DocPage` |
| GET | `/api/doc/pages/{id}` | — | `data: DocPage` |
| DELETE | `/api/doc/pages/{id}` | — | `{ "status":"ok" }` |
| GET | `/api/doc/pages/{id}/history` | — | `data: [DocPageVersion]` |
| GET | `/api/doc/pages/{id}/history/{version}` | — | `data: DocPageVersion` |

`POST /api/doc/pages`:
```jsonc
{ "project": "billing",          // slug | id | title (fuzzy, авто-создаётся)
  "page": "runbook-deploy",      // title (slug авто)
  "content": "markdown…",
  "tags": ["…"],
  "operation": "create|append|replace|section",   // default create
  "section_title": "…" }          // для operation=section
```

---

## Learning (треки → уроки)

| Метод | Путь | Тело / примечание | Ответ |
|---|---|---|---|
| GET | `/api/learning/tracks` | — | `data: [LearningTrack]` |
| POST | `/api/learning/tracks` | `{ "title", "raw_text"? | "file_path"?, "pace_hours"?, "tags"? }` | `201 data: Track` |
| GET | `/api/learning/tracks/{id}` | — | `data: Track` |
| DELETE | `/api/learning/tracks/{id}` | — | `{ "status":"ok" }` |
| GET | `/api/learning/tracks/{id}/lessons` | — | `data: [Lesson]` |
| POST | `/api/learning/tracks/{id}/next` | доставить следующий урок | `data: Lesson` |
| POST | `/api/learning/lessons/{id}/learned` | — | `data: Lesson` |
| POST | `/api/learning/lessons/{id}/skip` | — | `data: Lesson` |

---

## Graph

### `GET /api/graph/{id}?depth=<1..3>`
Граф связей вокруг записи: рёбра `semantic` (vector similarity) + `wiki` (`[[links]]`).
```jsonc
{ "status":"ok", "data": {
  "nodes": [ {"id","title","tags","score"?} ],
  "edges": [ {"from","to","kind":"semantic|wiki","score"?,"label"?} ]
}}
```

---

## Модели данных

```jsonc
Node        { id, kind, title, summary, status, tags:[], metadata:{}, created_at, updated_at }
NodeMember  { id, node_id, member_kind, member_id, role, label, created_at }
Note        { id, title, content, tags:[], created_at, updated_at }
Todo        { id, title, done, due_at?, created_at, completed_at? }
Reminder    { id, text, remind_at, fired, recurrence?, created_at }
Project     { id, slug, title, description, tags:[], created_at, updated_at }
DocPage     { id, project_id, slug, title, content, version, tags:[], created_at, updated_at }
DocPageVersion { id, page_id, version, title, content, tags:[], author, summary, created_at }
LearningTrack  { id, title, source_ref, total_lessons, current_lesson, pace_hours, last_delivered_at?, tags:[], created_at }
Lesson      { id, track_id, lesson_num, title, content, status, delivered_at?, learned_at?, created_at }
```
Все `id` — UUID-строки. Даты — RFC3339 UTC.

---

## Быстрый старт для интеграции

```bash
# проверить, что сервер жив
curl -sf http://localhost:8081/health

# состояние сервисов
curl -s http://localhost:8081/api/status | jq

# создать узел дежурства и привязать ссылку
NID=$(curl -s -XPOST localhost:8081/api/nodes -d '{"kind":"duty","title":"Дежурство billing","tags":["oncall"]}' | jq -r .data.id)
curl -s -XPOST localhost:8081/api/nodes/$NID/members \
  -d '{"kind":"link","member_id":"https://grafana/x","role":"dashboard","label":"Grafana"}'
curl -s localhost:8081/api/nodes/$NID | jq

# агентный ingest (нужен INGEST_AGENT=1 на сервере)
curl -s -XPOST localhost:8081/api/ingest \
  -d '{"raw":"заступаю на дежурство billing, сделай runbook деплоя, дашборд https://grafana/x"}' | jq

# подписка на живые события (в отдельном терминале)
curl -N localhost:8081/api/events
```
