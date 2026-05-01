# cozby-brain — agent loop (design)

> Текущий ingest — однократный вызов LLM, который классифицирует и
> структурирует ввод. Этого достаточно для «надо купить молоко», но
> мало для запросов вроде «найди ссылку из старой заметки про X»,
> «дополни эту страницу новым разделом», «составь программу тренировки
> на 4 недели по моим целям».
>
> Этот документ — план поэтапного перехода в агент-цикл.

## Что считаем «агентом» в cozby

LLM получает не один промпт, а **петлю** с доступом к набору **инструментов**.
На каждом шаге модель решает:

1. вызвать инструмент → получить результат → подумать дальше, или
2. дать финальный ответ пользователю + список изменений в БД.

Минимальный вид интерфейса для модели:

```
SYSTEM:  ты — личный AI-агент для cozby-brain. Ниже инструменты, на каждом
         шаге выбирай ОДИН: tool_call или final.

USER:    <reasoned-text-of-message>

(loop)
ASSISTANT (tool_call): {"tool":"search_notes","args":{"query":"ractor","k":5}}
TOOL_RESULT:           [{"id":"...","title":"Ractor 0.15","preview":"…"}]
ASSISTANT (tool_call): {"tool":"get_note","args":{"id":"…"}}
TOOL_RESULT:           {"id":"…","title":"…","content":"…"}
ASSISTANT (final):     {
  "reply": "Нашёл одну заметку про ractor — https://github.com/...",
  "actions": [
    { "type":"append_note", "id":"…", "content":"\\n\\nСм. также: …" }
  ]
}
```

## Инструменты (v1, минимальный набор)

Цель — закрыть use-case'ы из чата (workout-программа, поиск ссылок,
дополнение скудной заметки) без переусложнения.

| tool | args | назначение |
|---|---|---|
| `search` | `query: string, kind?: "note"\|"doc"\|"todo"\|"reminder"\|"all", k?: int` | RAG-поиск по embeddings с фильтром по типу |
| `get_note` | `id: string` | полный markdown заметки |
| `get_doc_page` | `id: string` | полный markdown страницы доки |
| `list_doc_pages` | `project: string` | оглавление проекта |
| `list_todos` | `status?: "done"\|"undone"\|"overdue"` | список задач |
| `list_reminders` | `status?: "fired"\|"pending"\|"overdue"` | список напоминаний |
| `now` | — | текущий UTC ISO-8601, чтобы LLM не выдумывал |

## Действия (что агент может изменить)

`final` сообщение содержит `actions[]`. Сервер их применяет в порядке,
без LLM-в-цикле, и возвращает результат пользователю.

| action | поля | эффект |
|---|---|---|
| `create_note` | `title, content, tags[]` | новая заметка |
| `append_note` | `id, content` | дописать в конец заметки |
| `update_note` | `id, content` | переписать целиком |
| `create_doc_page` | `project, page, content, tags[]` | новая страница |
| `append_doc_page` | `id, content` | дописать в конец |
| `create_todo` | `title, due_at?` | новая задача |
| `complete_todo` | `id` | пометить done |
| `create_reminder` | `text, remind_at, recurrence?` | новое напоминание (поддерживает recurring уже) |
| `create_doc_section` | `id, section_title, content` | добавить `## section_title` блок |

Все actions проходят через **одобрение пользователя** в TUI overlay перед
применением (`y` / `n`) — на старте без auto-apply, иначе агент натворит дел.

## Loop control

- **Maximum steps**: 6 (защита от зацикливания).
- **Total tool-call timeout**: 60 s.
- **Total LLM tokens budget**: 8K input + 4K output.
- На превышении лимита — отдать пользователю whatever-есть с пометкой
  «лимит шагов исчерпан».

## Где это живёт в коде

```
crates/agent/                    # новый крейт
├── src/
│   ├── lib.rs
│   ├── tools.rs                 # объявление инструментов + JSON-schema
│   ├── executor.rs              # цикл: model → tool → model → ...
│   ├── actions.rs               # применение `actions[]` через акторы
│   └── prompt.rs                # SYSTEM-промпт + few-shot
crates/application/src/ports.rs  # AgentRunner trait? или прямо актор
crates/web/src/handlers.rs       # POST /api/agent/run { raw } → SSE/JSON
```

Альтернатива — расширить существующий `classify_and_structure_with_context`,
вернув `actions[]`. Но это переусложнит use-case (он уже большой). Лучше
**второй endpoint**: ingest классифицирует short-form, agent отрабатывает
сложные запросы.

Триггер выбора: первый промпт-шаг — мини-классификатор `simple|agent`.
Простые («купи молоко») → старый ingest. Сложные («найди ссылку», «составь
программу», «дополни заметку про…») → agent.

## API

```
POST /api/agent/run
Body: {"raw": "<пользовательский запрос>"}

Response (когда model говорит final):
{
  "status": "ok",
  "reply": "string (markdown)",
  "actions": [ ... ],
  "trace": [
    {"tool":"search","args":{...},"result_preview":"…"},
    ...
  ]
}
```

TUI:
- В Inbox добавить кнопку/команду `:agent` или `;` для агентского режима.
- Ответ показывается как обычный preview.
- Если `actions.len() > 0` — overlay со списком предложенных изменений и
  `[A]pply all` / `[s]kip` / `[r]eview each`.

## Этапы внедрения

1. **Скелет**: новый крейт `cb-agent`, тип `Tool` + `Action`, executor с
   моком LLM, который всегда возвращает `final` без actions. Без UI.
2. **Read-only tools**: `search`, `get_note`, `get_doc_page`, `list_*`.
   Agent уже отвечает на вопросы со ссылками на источники.
3. **Apply step + overlay в TUI**: `create_*` / `append_*` actions с
   подтверждением.
4. **Few-shot для workout-программы**: примеры в системном промпте.
5. **Persistent agent state**: помнит результаты между сообщениями
   в одной сессии (для diff'ов и continuation).

## Почему пока не строим

- Без read-only слоя (этап 2) ценности мало — это просто переименование
  ingest. Read-only требует обвязки на repos и нового actor message.
- Apply (этап 3) добавит ~300-500 LoC и UI работу — отдельная сессия.
- Без few-shot модель будет промахиваться на сложных запросах вроде
  workout-программы — нужны 2-3 примера в промпте, тоже отдельный шаг.

Total: ~1.5-2 сессии чистой работы. Готов начать когда скажешь.

## Тест-кейсы для приёмки (use case из чата)

1. `составь программу тренировки на 3 дня <дальше идёт длинный список>`
   - `actions[0]` = `create_doc_page(project="fitness", page="программа тренировки", content="…")`
   - `actions[1..]` = `create_reminder(...)` для каждого дня недели с recurrence
2. `найди ссылку из заметки про ractor`
   - tool_calls: `search` → `get_note`
   - `reply` содержит URL + цитату
3. `дополни страницу X разделом про Y`
   - tool_calls: `list_doc_pages` → `get_doc_page`
   - `actions[0]` = `create_doc_section(...)`
4. `что я ещё забыл сделать сегодня?`
   - `list_todos(status=undone)` + `list_reminders(status=pending)`
   - `reply` = markdown-список
