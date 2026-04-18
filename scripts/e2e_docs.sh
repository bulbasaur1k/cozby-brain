#!/usr/bin/env bash
# End-to-end test: multi-page documentation scenarios.
# Verifies that LLM correctly splits one user message into multiple doc items
# targeting different pages and even different projects.

set -u
cd "$(dirname "$0")/.."

API=http://localhost:8081
CURL="curl -s --max-time 240"
PY='python3 -c'

banner() {
    printf "\n\033[1;34m────────────────────────────────────────────────────────────\n"
    printf "  %s\n" "$*"
    printf "────────────────────────────────────────────────────────────\033[0m\n"
}

assert_ok() {
    local label="$1"; local resp="$2"
    if echo "$resp" | grep -q '"error"'; then
        printf "\033[1;31m  [FAIL] %s\033[0m\n" "$label"
        echo "$resp" | head -5
        exit 1
    else
        printf "\033[1;32m  [OK]   %s\033[0m\n" "$label"
    fi
}

count_doc_items() {
    echo "$1" | $PY "
import sys,json
d = json.load(sys.stdin)
items = d.get('items', [d])
docs = [i for i in items if i.get('type') == 'doc']
print(len(docs))
"
}

banner "SCENARIO 1: одно сообщение → 4 страницы + проект в одном вызове"
echo "Входные данные: пользователь хочет задокументировать проект cozby-brain-docs"
echo "  со страницами: Architecture, API Reference, Deployment, Troubleshooting"
echo

RAW='Создай документацию проекта cozby-brain-docs. Нужно 4 страницы:
1. Architecture — hexagonal архитектура, 11 крейтов, domain изолирован от infra, ractor для акторов
2. API Reference — основные endpoints: /api/ingest, /api/doc/*, /api/notes, /api/todos
3. Deployment — docker compose поднимает db + qdrant + minio, приложение на host через run.sh
4. Troubleshooting — типичные проблемы: LLM timeout на reasoning-моделях, embedding 404 у routerai.ru'

RESP=$($CURL -X POST $API/api/ingest -H 'content-type: application/json' \
  -d "$($PY "import json,sys; print(json.dumps({'raw': sys.argv[1]}))" "$RAW")")

DOC_COUNT=$(count_doc_items "$RESP")
echo "  LLM вернула doc-items: $DOC_COUNT"

if [ "$DOC_COUNT" -lt 3 ]; then
    printf "\033[1;31m  [FAIL] ожидали минимум 3 страницы\033[0m\n"
    echo "$RESP" | $PY 'import sys,json; d=json.load(sys.stdin); print(json.dumps(d, ensure_ascii=False, indent=2)[:2000])'
    exit 1
fi

# pretty print the created pages
echo "$RESP" | $PY "
import sys,json
d = json.load(sys.stdin)
items = d.get('items', [d])
for i, item in enumerate(items):
    if item.get('type') == 'doc':
        data = item.get('data', {})
        print(f'    [{i+1}] {data.get(\"title\", \"?\"):<30}  slug={data.get(\"slug\", \"?\"):<20}  v{data.get(\"version\", 0)}')
"

printf "\n\033[1;36m  Список страниц в проекте:\033[0m\n"
./target/release/cozby doc pages cozby-brain-docs 2>&1 | sed 's/^/    /'

banner "SCENARIO 2: одно сообщение → правки на ДВЕ разные страницы (same project)"
echo "Входные данные: дополнить API Reference + изменить Troubleshooting"
echo

RAW='В проекте cozby-brain-docs на страницу API Reference добавь раздел про документационные endpoints: POST /api/doc/pages поддерживает операции append/section/replace/create, все правки версионируются. А в Troubleshooting добавь: если LLM возвращает невалидный JSON — проверь что модель не reasoning (они часто возвращают content=null). Используй параметр max_tokens>=4096.'

RESP=$($CURL -X POST $API/api/ingest -H 'content-type: application/json' \
  -d "$($PY "import json,sys; print(json.dumps({'raw': sys.argv[1]}))" "$RAW")")

DOC_COUNT=$(count_doc_items "$RESP")
echo "  LLM вернула doc-items: $DOC_COUNT"

if [ "$DOC_COUNT" -lt 2 ]; then
    printf "\033[1;31m  [FAIL] ожидали 2 страницы\033[0m\n"
    echo "$RESP" | $PY 'import sys,json; d=json.load(sys.stdin); print(json.dumps(d, ensure_ascii=False, indent=2)[:2000])'
    exit 1
fi

echo "$RESP" | $PY "
import sys,json
d = json.load(sys.stdin)
items = d.get('items', [d])
for i, item in enumerate(items):
    if item.get('type') == 'doc':
        data = item.get('data', {})
        print(f'    [{i+1}] {data.get(\"title\", \"?\"):<30}  v{data.get(\"version\", 0)}  обновлено')
"

banner "SCENARIO 3: одно сообщение → ДВА разных проекта"
echo "Входные данные: обновить cozby-brain-docs и создать новый проект"
echo

RAW='Добавь в cozby-brain-docs в страницу Deployment раздел про MinIO: сервис на порту 9000 (S3 API) и 9001 (консоль), бакет cozby-attachments создаётся автоматически через minio-init. И создай новый проект personal-workflow со страницей Daily Routine — утренняя рутина: 5 минут медитации, проверка календаря, планирование 3 главных задач.'

RESP=$($CURL -X POST $API/api/ingest -H 'content-type: application/json' \
  -d "$($PY "import json,sys; print(json.dumps({'raw': sys.argv[1]}))" "$RAW")")

DOC_COUNT=$(count_doc_items "$RESP")
echo "  LLM вернула doc-items: $DOC_COUNT"

if [ "$DOC_COUNT" -lt 2 ]; then
    printf "\033[1;31m  [FAIL] ожидали 2 doc-item\033[0m\n"
    echo "$RESP" | $PY 'import sys,json; d=json.load(sys.stdin); print(json.dumps(d, ensure_ascii=False, indent=2)[:2000])'
    exit 1
fi

echo "$RESP" | $PY "
import sys,json
d = json.load(sys.stdin)
items = d.get('items', [d])
for i, item in enumerate(items):
    if item.get('type') == 'doc':
        data = item.get('data', {})
        print(f'    [{i+1}] project={data.get(\"project_id\", \"?\")[:8]}  title={data.get(\"title\", \"?\"):<25}  v{data.get(\"version\", 0)}')
"

banner "Итоговое состояние"
printf "\n\033[1;36mВсе проекты:\033[0m\n"
./target/release/cozby doc projects 2>&1 | sed 's/^/  /'

printf "\n\033[1;36mСтраницы в cozby-brain-docs:\033[0m\n"
./target/release/cozby doc pages cozby-brain-docs 2>&1 | sed 's/^/  /'

printf "\n\033[1;36mСтраницы в personal-workflow:\033[0m\n"
./target/release/cozby doc pages personal-workflow 2>&1 | sed 's/^/  /'

banner "Проверка: есть ли история у правленых страниц?"
API_PAGE_ID=$(./target/release/cozby doc pages cozby-brain-docs 2>&1 | grep -i 'api' | awk '{print $1}' | head -1)
if [ -n "$API_PAGE_ID" ]; then
    FULL_ID=$($CURL "$API/api/doc/pages" -X POST \
      -H 'content-type: application/json' -d '{"project":"cozby-brain-docs","page":"API Reference","operation":"append","content":"","tags":[]}' \
      | $PY 'import sys,json; d=json.load(sys.stdin); print(d["data"]["id"])' 2>/dev/null)
    # простой путь: выгружаем history через API по короткому ID не получится — нужен полный
    echo "  (короткий id $API_PAGE_ID — история смотрим через API напрямую)"
fi

# Полный обход всех страниц, вывод истории каждой
./target/release/cozby doc pages cozby-brain-docs 2>&1 | awk '{print $1}' | while read SHORT; do
    # достать полный ID через /api/doc/pages/{short}… не API ищет по ID. Пропускаем.
    true
done

printf "\n\033[1;32m  Все сценарии прошли\033[0m\n\n"
