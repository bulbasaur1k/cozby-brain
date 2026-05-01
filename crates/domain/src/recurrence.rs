//! Recurrence-rule parsing и вычисление следующего срабатывания.
//!
//! Поддерживается узкое подмножество iCalendar RRULE — этого достаточно
//! для бытовых напоминаний и одновременно совместимо с экспортом в
//! календари (Apple/Google) когда дойдёт.
//!
//! Грамматика правила (case-insensitive, разделитель — `;`):
//!
//! ```text
//! FREQ=DAILY                       — каждый день
//! FREQ=DAILY;INTERVAL=2            — через день
//! FREQ=WEEKLY;BYDAY=MO             — каждый понедельник
//! FREQ=WEEKLY;BYDAY=MO,WE,FR       — пн/ср/пт
//! FREQ=MONTHLY;BYMONTHDAY=15       — 15-го числа каждого месяца
//! ```
//!
//! Что НЕ поддерживается (намеренно): COUNT, UNTIL, BYSETPOS, BYHOUR,
//! BYMINUTE — добавим если попросят. Для секунд/минут/часов используется
//! время из `remind_at`.

use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Timelike, Utc, Weekday};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freq {
    Daily,
    Weekly,
    Monthly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recurrence {
    pub freq: Freq,
    pub interval: u32,
    /// Для WEEKLY: дни недели; пустой = «как у предыдущего срабатывания».
    pub by_day: Vec<Weekday>,
    /// Для MONTHLY: число месяца (1..=31).
    pub by_month_day: Option<u32>,
}

#[derive(Debug, thiserror::Error)]
pub enum RecurrenceError {
    #[error("missing FREQ")]
    MissingFreq,
    #[error("invalid FREQ: {0}")]
    InvalidFreq(String),
    #[error("invalid INTERVAL: {0}")]
    InvalidInterval(String),
    #[error("invalid BYDAY: {0}")]
    InvalidByDay(String),
    #[error("invalid BYMONTHDAY: {0}")]
    InvalidByMonthDay(String),
}

pub fn parse(rule: &str) -> Result<Recurrence, RecurrenceError> {
    let mut freq: Option<Freq> = None;
    let mut interval: u32 = 1;
    let mut by_day: Vec<Weekday> = Vec::new();
    let mut by_month_day: Option<u32> = None;

    for part in rule.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let (k, v) = match part.split_once('=') {
            Some(kv) => kv,
            None => continue,
        };
        match k.to_ascii_uppercase().as_str() {
            "FREQ" => {
                freq = Some(match v.to_ascii_uppercase().as_str() {
                    "DAILY" => Freq::Daily,
                    "WEEKLY" => Freq::Weekly,
                    "MONTHLY" => Freq::Monthly,
                    other => return Err(RecurrenceError::InvalidFreq(other.into())),
                });
            }
            "INTERVAL" => {
                interval = v
                    .parse::<u32>()
                    .map_err(|_| RecurrenceError::InvalidInterval(v.into()))?;
                if interval == 0 {
                    return Err(RecurrenceError::InvalidInterval(v.into()));
                }
            }
            "BYDAY" => {
                by_day = v
                    .split(',')
                    .map(str::trim)
                    .filter(|d| !d.is_empty())
                    .map(parse_weekday)
                    .collect::<Result<_, _>>()?;
            }
            "BYMONTHDAY" => {
                let n: u32 = v
                    .parse()
                    .map_err(|_| RecurrenceError::InvalidByMonthDay(v.into()))?;
                if !(1..=31).contains(&n) {
                    return Err(RecurrenceError::InvalidByMonthDay(v.into()));
                }
                by_month_day = Some(n);
            }
            _ => {} // молча игнорируем неизвестные ключи (forward-compat)
        }
    }

    Ok(Recurrence {
        freq: freq.ok_or(RecurrenceError::MissingFreq)?,
        interval,
        by_day,
        by_month_day,
    })
}

fn parse_weekday(s: &str) -> Result<Weekday, RecurrenceError> {
    Ok(match s.to_ascii_uppercase().as_str() {
        "MO" => Weekday::Mon,
        "TU" => Weekday::Tue,
        "WE" => Weekday::Wed,
        "TH" => Weekday::Thu,
        "FR" => Weekday::Fri,
        "SA" => Weekday::Sat,
        "SU" => Weekday::Sun,
        other => return Err(RecurrenceError::InvalidByDay(other.into())),
    })
}

/// Следующее срабатывание правила СТРОГО после `after`, начиная с базового
/// времени `prev` (обычно — последний `remind_at`). Возвращает `None` если
/// правило не даёт допустимой даты в разумных рамках.
pub fn next_after(
    prev: DateTime<Utc>,
    rule: &Recurrence,
    after: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    // Безопасный потолок — 5 лет вперёд от after, чтобы не зациклиться на
    // «никогда не наступающих» правилах.
    let cap = after + Duration::days(366 * 5);
    let mut candidate = prev;
    let interval = rule.interval as i64;

    // Сдвигаем candidate вперёд, пока он не окажется > after.
    let mut steps = 0;
    loop {
        steps += 1;
        if steps > 5_000 {
            return None; // защита от багов в правиле
        }

        candidate = match rule.freq {
            Freq::Daily => candidate + Duration::days(interval),
            Freq::Weekly => weekly_step(candidate, rule),
            Freq::Monthly => monthly_step(candidate, rule),
        };

        if candidate > after {
            return Some(candidate);
        }
        if candidate > cap {
            return None;
        }
    }
}

fn weekly_step(prev: DateTime<Utc>, rule: &Recurrence) -> DateTime<Utc> {
    if rule.by_day.is_empty() {
        // Без BYDAY — каждые INTERVAL недель в тот же день.
        return prev + Duration::weeks(rule.interval as i64);
    }
    // Идём по дням, ищем следующий день недели из by_day.
    // INTERVAL для WEEKLY с BYDAY работает на уровне «недель цикла» — но в
    // нашем минимальном варианте мы трактуем INTERVAL=1 (по умолчанию) и
    // просто ищем ближайший подходящий день; INTERVAL>1 редко нужен в
    // быту, при необходимости расширим.
    let mut d = prev + Duration::days(1);
    while !rule.by_day.contains(&d.weekday()) {
        d += Duration::days(1);
    }
    d
}

fn monthly_step(prev: DateTime<Utc>, rule: &Recurrence) -> DateTime<Utc> {
    let day = rule.by_month_day.unwrap_or_else(|| prev.day());
    // следующий месяц того же года или январь следующего
    let (y, m) = (prev.year(), prev.month());
    let (next_y, next_m) = next_month(y, m, rule.interval);
    let day = clamp_day_to_month(next_y, next_m, day);
    let nd = NaiveDate::from_ymd_opt(next_y, next_m, day)
        .unwrap_or_else(|| NaiveDate::from_ymd_opt(next_y, next_m, 1).unwrap());
    let nt = nd.and_hms_opt(prev.hour(), prev.minute(), prev.second()).unwrap();
    Utc.from_utc_datetime(&nt)
}

fn next_month(y: i32, m: u32, interval: u32) -> (i32, u32) {
    let total = m as i32 + interval as i32 - 1; // 0-based total months
    let new_m = (total % 12) as u32 + 1;
    let new_y = y + total / 12;
    (new_y, new_m)
}

fn clamp_day_to_month(y: i32, m: u32, day: u32) -> u32 {
    // последний день месяца
    let last = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            // високосный?
            if NaiveDate::from_ymd_opt(y, 2, 29).is_some() {
                29
            } else {
                28
            }
        }
        _ => 31,
    };
    day.min(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dt(y: i32, m: u32, d: u32, h: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, 0, 0).unwrap()
    }

    #[test]
    fn parse_daily() {
        let r = parse("FREQ=DAILY").unwrap();
        assert_eq!(r.freq, Freq::Daily);
        assert_eq!(r.interval, 1);
    }

    #[test]
    fn parse_daily_interval() {
        let r = parse("FREQ=DAILY;INTERVAL=3").unwrap();
        assert_eq!(r.freq, Freq::Daily);
        assert_eq!(r.interval, 3);
    }

    #[test]
    fn parse_weekly_byday() {
        let r = parse("FREQ=WEEKLY;BYDAY=MO,WE,FR").unwrap();
        assert_eq!(r.freq, Freq::Weekly);
        assert_eq!(r.by_day, vec![Weekday::Mon, Weekday::Wed, Weekday::Fri]);
    }

    #[test]
    fn parse_monthly_bymonthday() {
        let r = parse("FREQ=MONTHLY;BYMONTHDAY=15").unwrap();
        assert_eq!(r.freq, Freq::Monthly);
        assert_eq!(r.by_month_day, Some(15));
    }

    #[test]
    fn parse_case_insensitive() {
        let r = parse("freq=daily;interval=2").unwrap();
        assert_eq!(r.freq, Freq::Daily);
        assert_eq!(r.interval, 2);
    }

    #[test]
    fn parse_missing_freq() {
        assert!(parse("INTERVAL=2").is_err());
    }

    #[test]
    fn next_after_daily_simple() {
        let prev = dt(2026, 4, 23, 9);
        let now = dt(2026, 4, 23, 9);
        let r = parse("FREQ=DAILY").unwrap();
        // prev уже в "now", следующий = +1 день
        assert_eq!(next_after(prev, &r, now), Some(dt(2026, 4, 24, 9)));
    }

    #[test]
    fn next_after_daily_skips_past() {
        // prev был неделю назад, now — сегодня. Должен догнать ровно один
        // следующий после now.
        let prev = dt(2026, 4, 16, 9);
        let now = dt(2026, 4, 23, 8);
        let r = parse("FREQ=DAILY").unwrap();
        assert_eq!(next_after(prev, &r, now), Some(dt(2026, 4, 23, 9)));
    }

    #[test]
    fn next_after_weekly_byday() {
        // Понедельник 20 апреля → BYDAY=WE → 22 апреля
        let prev = dt(2026, 4, 20, 9); // Mon
        let now = dt(2026, 4, 20, 9);
        let r = parse("FREQ=WEEKLY;BYDAY=WE").unwrap();
        assert_eq!(next_after(prev, &r, now), Some(dt(2026, 4, 22, 9))); // Wed
    }

    #[test]
    fn next_after_weekly_multi_byday() {
        // Со вторника BYDAY=MO,WE,FR → ближайший FR=Fri
        let prev = dt(2026, 4, 21, 9); // Tue
        let now = dt(2026, 4, 21, 9);
        let r = parse("FREQ=WEEKLY;BYDAY=MO,WE,FR").unwrap();
        // ожидаем среду 22-го
        assert_eq!(next_after(prev, &r, now), Some(dt(2026, 4, 22, 9)));
    }

    #[test]
    fn next_after_monthly_clamps_day() {
        // 31 января + 1 месяц = 28 февраля (не високосный)
        let prev = dt(2027, 1, 31, 9);
        let now = dt(2027, 1, 31, 9);
        let r = parse("FREQ=MONTHLY").unwrap();
        assert_eq!(next_after(prev, &r, now), Some(dt(2027, 2, 28, 9)));
    }

    #[test]
    fn next_after_monthly_bymonthday() {
        let prev = dt(2026, 4, 15, 9);
        let now = dt(2026, 4, 15, 9);
        let r = parse("FREQ=MONTHLY;BYMONTHDAY=15").unwrap();
        assert_eq!(next_after(prev, &r, now), Some(dt(2026, 5, 15, 9)));
    }
}
