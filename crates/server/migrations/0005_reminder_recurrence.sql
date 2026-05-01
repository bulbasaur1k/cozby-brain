-- Recurring reminders: RRULE-lite text column.
-- NULL = одноразовое напоминание (поведение по умолчанию, существующие записи).
-- Не-NULL = напоминание срабатывает, потом actor пересчитывает remind_at.
ALTER TABLE reminders ADD COLUMN IF NOT EXISTS recurrence TEXT;
