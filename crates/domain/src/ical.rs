//! Сериализация Reminder → iCalendar (RFC 5545).
//!
//! Цель — отдавать **subscribe-feed**: один файл `.ics` со всеми текущими
//! напоминаниями, который любой календарь умеет тянуть по URL и держать
//! в синхронизации. Это покрывает 80% кейса «события cozby в Apple/Google/
//! Outlook Calendar» без OAuth, CalDAV PUT, app-passwords и прочей возни.
//!
//! Поддерживается:
//! - VEVENT с DTSTART (UTC), SUMMARY, UID, DTSTAMP, CREATED, DURATION
//! - RRULE из `domain::recurrence` (DAILY/WEEKLY/MONTHLY)
//! - VALARM с DISPLAY-триггером в момент DTSTART
//!
//! НЕ поддерживается (намеренно, нам не нужно): VTODO, attendees,
//! attachments, X-* properties, EXDATE/EXRULE, complex BYSETPOS.

use crate::entities::Reminder;
use crate::recurrence::{self, Freq, Recurrence};
use chrono::{DateTime, Utc};
use std::fmt::Write as _;

const PRODID: &str = "-//cozby-brain//feed//EN";
const FEED_NAME: &str = "cozby-brain";

/// Сериализует список напоминаний в один VCALENDAR-документ.
pub fn calendar_feed(reminders: &[Reminder]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "BEGIN:VCALENDAR");
    let _ = writeln!(out, "VERSION:2.0");
    let _ = writeln!(out, "PRODID:{PRODID}");
    let _ = writeln!(out, "CALSCALE:GREGORIAN");
    let _ = writeln!(out, "METHOD:PUBLISH");
    let _ = writeln!(out, "X-WR-CALNAME:{FEED_NAME}");
    let _ = writeln!(out, "X-WR-CALDESC:Cozby reminders feed");

    for r in reminders {
        write_event(&mut out, r);
    }

    let _ = writeln!(out, "END:VCALENDAR");
    out
}

fn write_event(out: &mut String, r: &Reminder) {
    let _ = writeln!(out, "BEGIN:VEVENT");
    let _ = writeln!(out, "UID:{}@cozby-brain", r.id);
    let _ = writeln!(out, "DTSTAMP:{}", fmt_dt(Utc::now()));
    let _ = writeln!(out, "CREATED:{}", fmt_dt(r.created_at));
    let _ = writeln!(out, "LAST-MODIFIED:{}", fmt_dt(r.created_at));
    let _ = writeln!(out, "DTSTART:{}", fmt_dt(r.remind_at));
    let _ = writeln!(out, "DURATION:PT15M");
    let _ = writeln!(out, "SUMMARY:{}", escape_text(&r.text));

    if let Some(rule_str) = r.recurrence.as_deref().filter(|s| !s.is_empty()) {
        if let Ok(rule) = recurrence::parse(rule_str) {
            let _ = writeln!(out, "RRULE:{}", rrule_to_ical(&rule));
        }
    }

    // VALARM — нативное системное уведомление в DTSTART. На устройствах
    // пользователя календарь сам разбудит UI.
    let _ = writeln!(out, "BEGIN:VALARM");
    let _ = writeln!(out, "ACTION:DISPLAY");
    let _ = writeln!(out, "TRIGGER:PT0M");
    let _ = writeln!(out, "DESCRIPTION:{}", escape_text(&r.text));
    let _ = writeln!(out, "END:VALARM");

    let _ = writeln!(out, "END:VEVENT");
}

/// `Recurrence` → строка `FREQ=...;BYDAY=...` для свойства RRULE.
/// Наш RRULE-lite уже подмножество iCal — поэтому маппинг прямой.
pub fn rrule_to_ical(r: &Recurrence) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.push(format!(
        "FREQ={}",
        match r.freq {
            Freq::Daily => "DAILY",
            Freq::Weekly => "WEEKLY",
            Freq::Monthly => "MONTHLY",
        }
    ));
    if r.interval > 1 {
        parts.push(format!("INTERVAL={}", r.interval));
    }
    if !r.by_day.is_empty() {
        let days = r
            .by_day
            .iter()
            .map(weekday_to_ical)
            .collect::<Vec<_>>()
            .join(",");
        parts.push(format!("BYDAY={days}"));
    }
    if let Some(d) = r.by_month_day {
        parts.push(format!("BYMONTHDAY={d}"));
    }
    parts.join(";")
}

fn weekday_to_ical(w: &chrono::Weekday) -> &'static str {
    use chrono::Weekday::*;
    match w {
        Mon => "MO",
        Tue => "TU",
        Wed => "WE",
        Thu => "TH",
        Fri => "FR",
        Sat => "SA",
        Sun => "SU",
    }
}

/// UTC-форма iCalendar: YYYYMMDD'T'HHMMSS'Z'.
fn fmt_dt(dt: DateTime<Utc>) -> String {
    dt.format("%Y%m%dT%H%M%SZ").to_string()
}

/// Экранирование TEXT-значения по RFC 5545 §3.3.11.
/// Запятая, точка-с-запятой, обратный слеш, перенос строки.
fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\r' => {} // CR съедаем — RFC требует CRLF, но мы пишем LF и парсеры это принимают
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn dt(y: i32, m: u32, d: u32, h: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, 0, 0).unwrap()
    }

    fn sample_reminder(id: &str, text: &str, recurrence: Option<&str>) -> Reminder {
        Reminder {
            id: id.into(),
            text: text.into(),
            remind_at: dt(2026, 4, 23, 9),
            fired: false,
            recurrence: recurrence.map(|s| s.into()),
            created_at: dt(2026, 4, 20, 12),
        }
    }

    #[test]
    fn empty_feed_has_envelope() {
        let s = calendar_feed(&[]);
        assert!(s.starts_with("BEGIN:VCALENDAR"));
        assert!(s.contains("VERSION:2.0"));
        assert!(s.contains("PRODID:"));
        assert!(s.trim_end().ends_with("END:VCALENDAR"));
    }

    #[test]
    fn single_event_basic_fields() {
        let r = sample_reminder("abc-123", "выпить воды", None);
        let s = calendar_feed(&[r]);
        assert!(s.contains("BEGIN:VEVENT"));
        assert!(s.contains("UID:abc-123@cozby-brain"));
        assert!(s.contains("DTSTART:20260423T090000Z"));
        assert!(s.contains("DURATION:PT15M"));
        assert!(s.contains("SUMMARY:выпить воды"));
        assert!(s.contains("BEGIN:VALARM"));
        assert!(s.contains("END:VEVENT"));
        assert!(!s.contains("RRULE:"));
    }

    #[test]
    fn daily_recurrence_emits_rrule() {
        let r = sample_reminder("d", "таблетка", Some("FREQ=DAILY"));
        let s = calendar_feed(&[r]);
        assert!(s.contains("RRULE:FREQ=DAILY"));
    }

    #[test]
    fn weekly_byday_emits_rrule() {
        let r = sample_reminder("w", "тренировка", Some("FREQ=WEEKLY;BYDAY=MO,WE,FR"));
        let s = calendar_feed(&[r]);
        assert!(s.contains("RRULE:FREQ=WEEKLY;BYDAY=MO,WE,FR"));
    }

    #[test]
    fn monthly_emits_rrule() {
        let r = sample_reminder("m", "оплатить", Some("FREQ=MONTHLY;BYMONTHDAY=15"));
        let s = calendar_feed(&[r]);
        assert!(s.contains("RRULE:FREQ=MONTHLY;BYMONTHDAY=15"));
    }

    #[test]
    fn interval_omitted_when_one() {
        let rule = recurrence::parse("FREQ=DAILY;INTERVAL=1").unwrap();
        assert_eq!(rrule_to_ical(&rule), "FREQ=DAILY");
    }

    #[test]
    fn interval_emitted_when_more_than_one() {
        let rule = recurrence::parse("FREQ=DAILY;INTERVAL=3").unwrap();
        assert_eq!(rrule_to_ical(&rule), "FREQ=DAILY;INTERVAL=3");
    }

    #[test]
    fn escapes_special_chars_in_summary() {
        let r = sample_reminder("e", "купить: молоко, хлеб; см. список", None);
        let s = calendar_feed(&[r]);
        // запятая и точка-с-запятой должны быть экранированы
        assert!(s.contains("SUMMARY:купить: молоко\\, хлеб\\; см. список"));
    }

    #[test]
    fn newlines_in_text_become_backslash_n() {
        let r = sample_reminder("n", "first line\nsecond", None);
        let s = calendar_feed(&[r]);
        assert!(s.contains("SUMMARY:first line\\nsecond"));
    }

    #[test]
    fn multiple_events_each_have_envelope() {
        let r1 = sample_reminder("1", "a", None);
        let r2 = sample_reminder("2", "b", Some("FREQ=DAILY"));
        let s = calendar_feed(&[r1, r2]);
        assert_eq!(s.matches("BEGIN:VEVENT").count(), 2);
        assert_eq!(s.matches("END:VEVENT").count(), 2);
        assert_eq!(s.matches("RRULE:").count(), 1);
    }

    #[test]
    fn invalid_recurrence_silently_dropped() {
        // Если в БД оказался мусор — феед всё равно валиден, просто
        // событие выходит как одноразовое.
        let r = sample_reminder("bad", "x", Some("garbage=foo"));
        let s = calendar_feed(&[r]);
        assert!(!s.contains("RRULE:"));
        assert!(s.contains("BEGIN:VEVENT"));
    }
}
