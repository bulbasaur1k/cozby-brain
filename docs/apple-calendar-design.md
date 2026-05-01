# cozby-brain ↔ Apple Calendar (design)

> Цель: рекуррентные/одноразовые reminders из cozby попадают как события
> в стандартный Apple Calendar (iCloud), и видны в нативных приложениях
> на macOS, iOS, iPadOS. Двусторонняя синхронизация — на этап 2.

## Почему CalDAV, а не Google Calendar API

iCloud отдаёт календари по **CalDAV** — открытому HTTP-протоколу поверх
WebDAV. Не нужны OAuth-flow и Apple developer ID; авторизация — обычный
Basic Auth с **app-specific password** (генерируется на appleid.apple.com).
Тот же CalDAV ходит в Nextcloud, Fastmail и десятки других сервисов —
один клиент = много провайдеров «бесплатно».

Google Calendar тоже умеет CalDAV (read-only) и REST API (полноценно).
REST требует OAuth2 → отдельная история, отложим.

## Что нужно знать о CalDAV конкретно для iCloud

- Endpoint: `https://caldav.icloud.com`
- Auth: Basic, `Apple ID email : app-specific password`
- Discovery: `PROPFIND /` → `<current-user-principal>` → URL principal'а →
  `PROPFIND` на нём → `<calendar-home-set>` → URL контейнера календарей →
  `PROPFIND` на контейнере (Depth: 1) → список календарей.
- Создание события: `PUT <calendar-url>/<uid>.ics` с `iCalendar (.ics)` body.
- Обновление: `PUT` с тем же `<uid>.ics` (CalDAV использует ETag для
  optimistic concurrency).
- Удаление: `DELETE <calendar-url>/<uid>.ics`.
- Получение списка событий: `REPORT <calendar-url>` (calendar-query).

Все нюансы в RFC 4791 (CalDAV) и RFC 5545 (iCalendar).

## Конфиг в .env

```env
# CalDAV
APPLE_CALDAV_URL=https://caldav.icloud.com
APPLE_CALDAV_USER=user@icloud.com
APPLE_CALDAV_APP_PASSWORD=xxxx-xxxx-xxxx-xxxx
# Какой именно календарь использовать (по DAV:displayname). Если не задан —
# создадим/найдём «cozby-brain» автоматом.
APPLE_CALENDAR_NAME=cozby-brain
# Включает sync. По умолчанию off — без ключа фича безопасно молчит.
APPLE_CALENDAR_SYNC=true
```

## Маппинг cozby → iCalendar

`Reminder` (в т.ч. recurring) → `VEVENT`:

| cozby | VEVENT |
|---|---|
| `id` | `UID` |
| `text` | `SUMMARY` |
| `remind_at` | `DTSTART` (UTC) — для одноразовых |
| `recurrence` (RRULE-lite) | `RRULE` (наш RRULE-lite уже совместим с iCal) |
| `created_at` | `CREATED`, `DTSTAMP` |
| `fired` (одноразовые) | helper-флаг для skip при экспорте |

`DURATION:PT15M` ставим всегда — иначе клиенты показывают «весь день».

`VALARM` блок добавляем для нативного push:
```
BEGIN:VALARM
ACTION:DISPLAY
TRIGGER:PT0M
DESCRIPTION:cozby reminder
END:VALARM
```

`Todo` с `due_at` → `VTODO` (более точное соответствие, но Apple Calendar
показывает их в Reminders.app, не в Calendar.app). На старте мапим `Todo`
тоже в `VEVENT` — будет видно в основном календаре. Если понадобится
правильный VTODO — добавим переключатель.

## Архитектура

Новый крейт `cb-calendar`:

```
crates/calendar/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── caldav.rs           # минимальный CalDAV клиент (PROPFIND, PUT, DELETE, REPORT)
    ├── ical.rs             # сериализация Reminder/Todo → VEVENT/iCalendar text
    └── apple_sync.rs       # высокоуровневый Sync: discover calendar, push, prune
```

Новые акторы / интеграция в существующие:

- **Вариант A** (минимум): после `ReminderActor::Create`/`Delete` /
  `set_fired` синхронно делать `apple_sync.push(reminder)` через handle.
  Минусы: блокирует actor если CalDAV лагает.
- **Вариант B** (предпочитаемый): отдельный `CalendarSyncActor`, акторы
  reminder/todo шлют ему `cast!` сообщения (`Push(reminder)`, `Remove(id)`).
  Sync актор группирует, ретраит, batch'ит. Не блокирует домен.

Идём по B.

## Ports

```rust
// в application/src/ports.rs
pub enum CalendarEvent {
    Upsert(Event),  // Event = id, summary, start, rrule, alarm, ...
    Remove(String), // by id
}

#[async_trait]
pub trait CalendarSync: Send + Sync {
    async fn upsert(&self, event: &Event) -> Result<(), CalendarError>;
    async fn remove(&self, uid: &str) -> Result<(), CalendarError>;
    /// Идемпотентная инициализация — discover/create календарь.
    async fn ensure_ready(&self) -> Result<(), CalendarError>;
    fn name(&self) -> &'static str; // "apple", "noop"
}
```

NoopSync — когда `APPLE_CALENDAR_SYNC=false`. Никогда не падает.

## Этапы

1. **Skeleton crate + iCalendar serializer**: только сериализация, без
   сети. Тесты: VEVENT + RRULE + VALARM соответствуют примерам из RFC.
2. **CalDAV client**: PROPFIND discovery, PUT/DELETE для одного календаря.
   Интеграционный тест на mock HTTP-сервере (`wiremock` или ручной axum).
3. **AppleCalDAVSync**: ensure_ready → discover/find/create календарь по
   `APPLE_CALENDAR_NAME`. Реальный smoke против iCloud за app-password.
4. **CalendarSyncActor**: подписан на reminder/todo события через cast,
   ретраит на 5xx, дропает на 4xx с warn.
5. **Reverse sync (этап 2 на потом)**: REPORT calendar-query → diff с БД,
   импорт изменений извне. Сложнее (conflict resolution).

## Безопасность

- App-password живёт ТОЛЬКО в `.env` — не в БД, не в git, не в логах.
- Логи маскируют basic-auth (как `mask_db_url` маскирует password в DB).
- Нет UI для ввода пароля — пользователь сам редактирует .env. Так
  никто не сможет попросить агента «вытяни пароль».

## Тест-кейсы

1. **Recurring**: `FREQ=DAILY` reminder из cozby → событие в Calendar.app
   повторяется каждый день, alarm срабатывает за 0 минут. После
   `complete_todo` (когда появится для todo) — событие исчезает.
2. **Edit**: меняем `text` или `remind_at` в cozby → следующий tick
   sync-актора `PUT`'ит новый VEVENT (тот же UID), Calendar.app обновляет.
3. **Delete**: `DELETE` reminder → `DELETE <uid>.ics` → пропадает на
   всех устройствах через iCloud push.
4. **Resilience**: CalDAV возвращает 5xx 3 раза подряд → синк актор
   ставит событие в очередь и ретраит, не теряет.

## Оценка трудозатрат

- iCalendar serializer + RRULE mapping: ½ дня
- CalDAV клиент (PROPFIND/PUT/DELETE/REPORT, парсинг multistatus XML): 1 день
- AppleCalDAVSync (discovery + бизнес-логика): ½ дня
- CalendarSyncActor + интеграция: ½ дня
- Тесты + smoke на iCloud: ½ дня

**Итого: ~3 дня**, плюс 0-2 дня на отладку под конкретного юзера
(сертификаты, MitM-прокси, корп-сеть).

## Открытые вопросы (под пользователя)

- **Однонаправленный экспорт** (cozby → Calendar) на старте достаточно,
  или сразу нужен импорт изменений из календаря в cozby?
- **Какой календарь по умолчанию**: создавать ли отдельный
  `cozby-brain`, или сливать в существующий `Reminders` / личный
  календарь? По умолчанию предлагаю отдельный — проще undo.
- **Выгружать ли todo** (как VEVENT или VTODO)? VTODO правильнее, но
  показывается только в Reminders.app, не в Calendar.app.
- **Уведомления**: дублировать ли наш notify-rust popup и Apple alarm,
  или при включённом sync выключать desktop popup, оставляя только
  Apple-овский? Скорее второе — иначе двойная нотификация.
