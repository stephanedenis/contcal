//! Minimal iCalendar (RFC 5545) parser and recurrence expander.
//!
//! The module only depends on `chrono`.  It is intentionally forgiving: a
//! malformed `VEVENT` is skipped with a warning instead of making the whole
//! calendar unusable.

use chrono::{
    Datelike, Days, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Weekday,
};
use std::collections::{BTreeSet, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByDay {
    /// 0 = every such weekday, 1 = first, 2 = second, -1 = last, ...
    pub ordinal: i8,
    pub weekday: Weekday,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recurrence {
    pub freq: Freq,
    pub interval: u32,
    pub count: Option<u32>,
    pub until: Option<NaiveDate>,
    pub by_day: Vec<ByDay>,
    pub by_month_day: Vec<i32>,
    pub by_month: Vec<u32>,
    pub wkst: Weekday,
}

#[derive(Debug, Clone)]
pub struct Event {
    pub uid: String,
    pub summary: String,
    #[allow(dead_code)]
    pub location: Option<String>,
    /// First day of the event (local wall-clock date).
    pub start: NaiveDate,
    /// Local start time for timed events; None for all-day events.
    pub start_time: Option<NaiveTime>,
    /// Number of calendar days the event covers, always >= 1.
    /// All-day Jan 1 -> Jan 2 (DTEND exclusive) = 1.
    /// Timed 23:00 Jan 1 -> 01:00 Jan 2 = 2.
    pub span_days: i64,
    pub all_day: bool,
    pub rrule: Option<Recurrence>,
    /// Dates removed via EXDATE (date granularity).
    pub exdates: HashSet<NaiveDate>,
    /// Extra dates via RDATE (date granularity).
    pub rdates: Vec<NaiveDate>,
    /// Set for a modified recurrence instance (RECURRENCE-ID).
    pub recurrence_id: Option<NaiveDate>,
}

#[derive(Debug, Clone, Default)]
pub struct ParsedCalendar {
    pub events: Vec<Event>,
    pub warnings: Vec<String>,
}

/// Parse an ICS document. Forgiving: skip broken VEVENTs with a warning,
/// tolerate CRLF/LF, line folding, BOM, unknown properties/components.
pub fn parse_ics(input: &str) -> ParsedCalendar {
    let mut parsed = ParsedCalendar::default();
    let mut warnings: Vec<String> = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    // Current VEVENT: depth in `stack` and its direct properties.
    let mut current: Option<(usize, Vec<RawProp>)> = None;
    let mut event_index: usize = 0;

    for line in unfold_lines(input) {
        let Some(prop) = parse_content_line(&line) else {
            continue;
        };

        match prop.name.as_str() {
            "BEGIN" => {
                let component = prop.value.trim().to_ascii_uppercase();
                if component.is_empty() {
                    continue;
                }
                stack.push(component.clone());
                if component == "VEVENT" && current.is_none() {
                    current = Some((stack.len(), Vec::new()));
                }
            }
            "END" => {
                let component = prop.value.trim().to_ascii_uppercase();
                if component == "VEVENT" {
                    let is_our_event = current
                        .as_ref()
                        .map(|(depth, _)| *depth == stack.len())
                        .unwrap_or(false);
                    if is_our_event && let Some((_, props)) = current.take() {
                        if let Some(event) = build_event(&props, event_index, &mut warnings) {
                            parsed.events.push(event);
                        }
                        event_index += 1;
                    }
                }
                if stack.last().map(|top| top == &component).unwrap_or(false) {
                    stack.pop();
                } else if let Some(pos) = stack.iter().rposition(|c| c == &component) {
                    stack.truncate(pos);
                }
            }
            _ => {
                let at_event_level = stack.last().map(|top| top == "VEVENT").unwrap_or(false)
                    && current
                        .as_ref()
                        .map(|(depth, _)| *depth == stack.len())
                        .unwrap_or(false);
                if at_event_level && let Some((_, props)) = current.as_mut() {
                    props.push(prop);
                }
            }
        }
    }

    // Recurrence overrides hide their base instance.
    let overrides: Vec<(String, NaiveDate)> = parsed
        .events
        .iter()
        .filter_map(|event| event.recurrence_id.map(|rid| (event.uid.clone(), rid)))
        .collect();
    if !overrides.is_empty() {
        for event in &mut parsed.events {
            if event.rrule.is_some() && event.recurrence_id.is_none() {
                for (uid, rid) in &overrides {
                    if uid == &event.uid {
                        event.exdates.insert(*rid);
                    }
                }
            }
        }
    }

    // Stable, short warning list (French, for the UI).
    let mut seen = HashSet::new();
    parsed.warnings = warnings
        .into_iter()
        .filter(|warning| seen.insert(warning.clone()))
        .take(20)
        .collect();
    parsed
}

/// Whether the body looks like an iCalendar document (for a helpful fetch error).
pub fn looks_like_ics(text: &str) -> bool {
    let trimmed = text.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    let lower = trimmed.to_ascii_lowercase();
    if lower.contains("<html") || lower.contains("<!doctype html") {
        return false;
    }
    lower.contains("begin:vcalendar")
}

/// Occurrence START dates whose covered interval [start, start+span_days)
/// intersects the inclusive date range [from, to].
/// - Applies EXDATE and RDATE.
/// - Non-recurring events are returned only if they intersect.
/// - Results sorted ascending and deduplicated.
/// - Must terminate quickly for old daily recurring events (cap iterations safely).
pub fn occurrence_days(event: &Event, from: NaiveDate, to: NaiveDate) -> Vec<NaiveDate> {
    if to < from {
        return Vec::new();
    }
    let span = event.span_days.max(1) as u64;

    let mut dates = raw_occurrences(event, from, to);
    for rdate in &event.rdates {
        dates.insert(*rdate);
    }
    for exdate in &event.exdates {
        dates.remove(exdate);
    }

    dates
        .into_iter()
        .filter(|start| intersects(*start, from, to, span))
        .collect()
}

fn intersects(start: NaiveDate, from: NaiveDate, to: NaiveDate, span: u64) -> bool {
    if start > to {
        return false;
    }
    match start.checked_add_days(Days::new(span.max(1))) {
        Some(end) => end > from,
        None => true,
    }
}

// ---------------------------------------------------------------------------
// Content-line parsing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct RawProp {
    name: String,
    params: Vec<(String, String)>,
    value: String,
}

impl RawProp {
    fn param(&self, key: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }
}

fn find_unquoted_char(text: &str, needle: char) -> Option<usize> {
    let mut in_quotes = false;
    for (index, ch) in text.char_indices() {
        if ch == '"' {
            in_quotes = !in_quotes;
        } else if ch == needle && !in_quotes {
            return Some(index);
        }
    }
    None
}

fn split_unquoted(text: &str, delimiter: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut in_quotes = false;
    let mut start = 0;
    for (index, ch) in text.char_indices() {
        if ch == '"' {
            in_quotes = !in_quotes;
        } else if ch == delimiter && !in_quotes {
            parts.push(&text[start..index]);
            start = index + ch.len_utf8();
        }
    }
    parts.push(&text[start..]);
    parts
}

/// Split into logical lines, removing BOM and unfolding continuations.
fn unfold_lines(input: &str) -> Vec<String> {
    let input = input.strip_prefix('\u{feff}').unwrap_or(input);
    let mut physical: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                physical.push(std::mem::take(&mut current));
            }
            '\n' => physical.push(std::mem::take(&mut current)),
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        physical.push(current);
    }

    let mut logical: Vec<String> = Vec::new();
    for line in physical {
        if let Some(rest) = line.strip_prefix([' ', '\t'])
            && let Some(previous) = logical.last_mut()
        {
            previous.push_str(rest);
            continue;
        }
        logical.push(line);
    }
    logical
}

fn parse_content_line(line: &str) -> Option<RawProp> {
    let colon = find_unquoted_char(line, ':')?;
    let left = &line[..colon];
    let value = line[colon + 1..].to_string();

    let segments = split_unquoted(left, ';');
    let name = segments.first()?.trim().to_ascii_uppercase();
    if name.is_empty() {
        return None;
    }

    let mut params = Vec::new();
    for segment in segments.iter().skip(1) {
        let Some(eq) = find_unquoted_char(segment, '=') else {
            continue;
        };
        let key = segment[..eq].trim().to_ascii_uppercase();
        let raw_value = segment[eq + 1..].trim();
        if key.is_empty() {
            continue;
        }
        let param_value = raw_value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .unwrap_or(raw_value)
            .to_string();
        params.push((key, param_value));
    }

    Some(RawProp {
        name,
        params,
        value,
    })
}

// ---------------------------------------------------------------------------
// Value parsing helpers
// ---------------------------------------------------------------------------

fn parse_digits(text: &str) -> Option<u32> {
    if text.is_empty() || !text.is_ascii() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse::<u32>().ok()
}

fn parse_compact_date(value: &str) -> Option<NaiveDate> {
    let value = value.trim();
    if value.len() != 8 || !value.is_ascii() {
        return None;
    }
    let year = parse_digits(&value[0..4])?;
    let month = parse_digits(&value[4..6])?;
    let day = parse_digits(&value[6..8])?;
    NaiveDate::from_ymd_opt(year as i32, month, day)
}

/// Parse `YYYYMMDDTHHMMSS` (and, tolerantly, `YYYYMMDDTHHMM`).
/// Returns `(datetime, is_utc)`.
fn parse_datetime_value(value: &str) -> Option<(NaiveDateTime, bool)> {
    let value = value.trim();
    if value.is_empty() || !value.is_ascii() {
        return None;
    }
    let (body, utc) = match value.strip_suffix('Z').or_else(|| value.strip_suffix('z')) {
        Some(body) => (body, true),
        None => (value, false),
    };
    let t_pos = body.find('T').or_else(|| body.find('t'))?;
    let date = parse_compact_date(&body[..t_pos])?;
    let time_part = &body[t_pos + 1..];
    let (hour, minute, second) = match time_part.len() {
        4 => (
            parse_digits(&time_part[0..2])?,
            parse_digits(&time_part[2..4])?,
            0,
        ),
        6 => (
            parse_digits(&time_part[0..2])?,
            parse_digits(&time_part[2..4])?,
            parse_digits(&time_part[4..6])?,
        ),
        _ => return None,
    };
    let time = NaiveTime::from_hms_opt(hour, minute, second)?;
    Some((date.and_time(time), utc))
}

/// Convert a parsed datetime to the local wall-clock date/time.
fn local_parts(datetime: NaiveDateTime, utc: bool) -> (NaiveDate, NaiveTime) {
    if utc {
        let local = Local.from_utc_datetime(&datetime);
        (local.date_naive(), local.time())
    } else {
        (datetime.date(), datetime.time())
    }
}

fn local_date(datetime: NaiveDateTime, utc: bool) -> NaiveDate {
    local_parts(datetime, utc).0
}

/// Parse a DATE or DATETIME value and reduce it to a local date.
fn parse_date_token(value: &str) -> Option<NaiveDate> {
    let value = value.trim();
    if value.is_empty() || value.contains('/') {
        return None;
    }
    if value.len() == 8 && value.is_ascii() && value.bytes().all(|b| b.is_ascii_digit()) {
        return parse_compact_date(value);
    }
    let (datetime, utc) = parse_datetime_value(value)?;
    Some(local_date(datetime, utc))
}

fn parse_weekday(value: &str) -> Option<Weekday> {
    match value.trim().to_ascii_uppercase().as_str() {
        "MO" => Some(Weekday::Mon),
        "TU" => Some(Weekday::Tue),
        "WE" => Some(Weekday::Wed),
        "TH" => Some(Weekday::Thu),
        "FR" => Some(Weekday::Fri),
        "SA" => Some(Weekday::Sat),
        "SU" => Some(Weekday::Sun),
        _ => None,
    }
}

fn unescape_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') | Some('N') => out.push('\n'),
                Some(',') => out.push(','),
                Some(';') => out.push(';'),
                Some('\\') => out.push('\\'),
                Some(other) => out.push(other),
                None => {}
            }
        } else if ch == '\n' || !ch.is_control() {
            out.push(ch);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// VEVENT building
// ---------------------------------------------------------------------------

struct StartInfo {
    date: NaiveDate,
    time: Option<NaiveTime>,
    /// Local wall-clock datetime used to apply DURATION (midnight for all-day).
    datetime: NaiveDateTime,
    all_day: bool,
}

fn find_prop<'a>(props: &'a [RawProp], name: &str) -> Option<&'a RawProp> {
    props.iter().find(|prop| prop.name == name)
}

fn parse_start(prop: &RawProp) -> Option<StartInfo> {
    let value = prop.value.trim();
    let param_is_date = prop
        .param("VALUE")
        .map(|v| v.eq_ignore_ascii_case("DATE"))
        .unwrap_or(false);
    let looks_like_date =
        value.len() == 8 && value.is_ascii() && value.bytes().all(|b| b.is_ascii_digit());

    if param_is_date || looks_like_date {
        let date = parse_compact_date(value)?;
        Some(StartInfo {
            date,
            time: None,
            datetime: date.and_time(NaiveTime::MIN),
            all_day: true,
        })
    } else {
        let (datetime, utc) = parse_datetime_value(value)?;
        let (date, time) = local_parts(datetime, utc);
        Some(StartInfo {
            date,
            time: Some(time),
            datetime: date.and_time(time),
            all_day: false,
        })
    }
}

fn parse_end_date(prop: &RawProp) -> Option<NaiveDate> {
    let value = prop.value.trim();
    let param_is_date = prop
        .param("VALUE")
        .map(|v| v.eq_ignore_ascii_case("DATE"))
        .unwrap_or(false);
    let looks_like_date =
        value.len() == 8 && value.is_ascii() && value.bytes().all(|b| b.is_ascii_digit());

    if param_is_date || looks_like_date {
        parse_compact_date(value)
    } else {
        let (datetime, utc) = parse_datetime_value(value)?;
        Some(local_date(datetime, utc))
    }
}

fn parse_duration(value: &str) -> Option<i64> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let (sign, body) = if let Some(rest) = value.strip_prefix('-') {
        (-1i64, rest)
    } else if let Some(rest) = value.strip_prefix('+') {
        (1i64, rest)
    } else {
        (1i64, value)
    };

    let mut chars = body.chars();
    match chars.next() {
        Some('P') | Some('p') => {}
        _ => return None,
    }

    let mut seconds: i64 = 0;
    let mut in_time = false;
    let mut saw_component = false;
    let mut number: Option<i64> = None;

    for ch in chars {
        match ch {
            '0'..='9' => {
                let digit = (ch as u8 - b'0') as i64;
                number = Some(number.unwrap_or(0).checked_mul(10)?.checked_add(digit)?);
            }
            'T' | 't' => {
                if in_time || number.is_some() {
                    return None;
                }
                in_time = true;
            }
            'W' | 'w' => {
                if in_time {
                    return None;
                }
                let n = number.take()?;
                seconds = seconds.checked_add(n.checked_mul(604_800)?)?;
                saw_component = true;
            }
            'D' | 'd' => {
                if in_time {
                    return None;
                }
                let n = number.take()?;
                seconds = seconds.checked_add(n.checked_mul(86_400)?)?;
                saw_component = true;
            }
            'H' | 'h' => {
                if !in_time {
                    return None;
                }
                let n = number.take()?;
                seconds = seconds.checked_add(n.checked_mul(3_600)?)?;
                saw_component = true;
            }
            'M' | 'm' => {
                if !in_time {
                    return None;
                }
                let n = number.take()?;
                seconds = seconds.checked_add(n.checked_mul(60)?)?;
                saw_component = true;
            }
            'S' | 's' => {
                if !in_time {
                    return None;
                }
                let n = number.take()?;
                seconds = seconds.checked_add(n)?;
                saw_component = true;
            }
            _ => return None,
        }
    }

    if number.is_some() || !saw_component {
        return None;
    }
    sign.checked_mul(seconds)
}

fn build_event(props: &[RawProp], index: usize, warnings: &mut Vec<String>) -> Option<Event> {
    if find_prop(props, "STATUS")
        .map(|prop| {
            let status = prop.value.trim();
            status.eq_ignore_ascii_case("CANCELLED") || status.eq_ignore_ascii_case("CANCELED")
        })
        .unwrap_or(false)
    {
        return None;
    }

    let dtstart = match find_prop(props, "DTSTART") {
        Some(prop) => prop,
        None => {
            warnings.push("VEVENT ignoré : DTSTART manquant".to_string());
            return None;
        }
    };
    let start = match parse_start(dtstart) {
        Some(start) => start,
        None => {
            warnings.push("VEVENT ignoré : DTSTART invalide".to_string());
            return None;
        }
    };

    let mut span_days: i64 = 1;
    if let Some(dtend) = find_prop(props, "DTEND") {
        let Some(end_date) = parse_end_date(dtend) else {
            warnings.push("VEVENT ignoré : DTEND invalide".to_string());
            return None;
        };
        span_days = if start.all_day {
            (end_date - start.date).num_days().max(1)
        } else {
            ((end_date - start.date).num_days() + 1).max(1)
        };
    } else if let Some(duration_prop) = find_prop(props, "DURATION") {
        match parse_duration(&duration_prop.value) {
            Some(seconds) => {
                span_days = if start.all_day {
                    (seconds / 86_400).max(1)
                } else {
                    match Duration::try_seconds(seconds)
                        .and_then(|duration| start.datetime.checked_add_signed(duration))
                    {
                        Some(end) => ((end.date() - start.date).num_days() + 1).max(1),
                        None => 1,
                    }
                };
            }
            None => warnings.push("DURATION ignorée : valeur invalide".to_string()),
        }
    }

    let summary = find_prop(props, "SUMMARY")
        .map(|prop| unescape_text(&prop.value))
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "(sans titre)".to_string());

    let location = find_prop(props, "LOCATION")
        .map(|prop| unescape_text(&prop.value))
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty());

    let uid = find_prop(props, "UID")
        .map(|prop| prop.value.trim().to_string())
        .filter(|uid| !uid.is_empty())
        .unwrap_or_else(|| format!("no-uid-{index}"));

    let rrule = find_prop(props, "RRULE").and_then(|prop| parse_rrule(&prop.value, warnings));

    let mut exdates = HashSet::new();
    for prop in props.iter().filter(|prop| prop.name == "EXDATE") {
        for token in split_unquoted(&prop.value, ',') {
            let token = token.trim();
            if token.is_empty() || token.contains('/') {
                continue;
            }
            match parse_date_token(token) {
                Some(date) => {
                    exdates.insert(date);
                }
                None => warnings.push("EXDATE ignoré : date invalide".to_string()),
            }
        }
    }

    let mut rdates = Vec::new();
    for prop in props.iter().filter(|prop| prop.name == "RDATE") {
        for token in split_unquoted(&prop.value, ',') {
            let token = token.trim();
            if token.is_empty() || token.contains('/') {
                // PERIOD values are not supported for calendar-day expansion.
                continue;
            }
            match parse_date_token(token) {
                Some(date) => {
                    if !rdates.contains(&date) {
                        rdates.push(date);
                    }
                }
                None => warnings.push("RDATE ignoré : date invalide".to_string()),
            }
        }
    }

    let recurrence_id = match find_prop(props, "RECURRENCE-ID") {
        Some(prop) => match parse_date_token(&prop.value) {
            Some(date) => Some(date),
            None => {
                warnings.push("RECURRENCE-ID ignorée : date invalide".to_string());
                None
            }
        },
        None => None,
    };

    Some(Event {
        uid,
        summary,
        location,
        start: start.date,
        start_time: start.time,
        span_days,
        all_day: start.all_day,
        rrule,
        exdates,
        rdates,
        recurrence_id,
    })
}

// ---------------------------------------------------------------------------
// RRULE parsing
// ---------------------------------------------------------------------------

fn parse_freq(value: &str) -> Option<Freq> {
    match value.trim().to_ascii_uppercase().as_str() {
        "DAILY" => Some(Freq::Daily),
        "WEEKLY" => Some(Freq::Weekly),
        "MONTHLY" => Some(Freq::Monthly),
        "YEARLY" => Some(Freq::Yearly),
        _ => None,
    }
}

fn parse_by_day_item(item: &str) -> Option<ByDay> {
    let item = item.trim();
    if item.len() < 2 || !item.is_ascii() {
        return None;
    }
    let weekday = parse_weekday(&item[item.len() - 2..])?;
    let ordinal_part = &item[..item.len() - 2];
    let ordinal = if ordinal_part.is_empty() {
        0
    } else {
        let value = ordinal_part.parse::<i32>().ok()?;
        if !(-53..=53).contains(&value) {
            return None;
        }
        value as i8
    };
    Some(ByDay { ordinal, weekday })
}

fn parse_until_value(value: &str) -> Option<NaiveDate> {
    parse_date_token(value)
}

fn parse_rrule(value: &str, warnings: &mut Vec<String>) -> Option<Recurrence> {
    let mut freq = None;
    let mut interval: u32 = 1;
    let mut count = None;
    let mut until = None;
    let mut by_day: Vec<ByDay> = Vec::new();
    let mut by_month_day: Vec<i32> = Vec::new();
    let mut by_month: Vec<u32> = Vec::new();
    let mut wkst = Weekday::Mon;

    for part in value.split(';') {
        let Some((key, raw)) = part.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_uppercase();
        let raw = raw.trim();
        match key.as_str() {
            "FREQ" => {
                freq = parse_freq(raw);
                if freq.is_none() {
                    warnings
                        .push("RRULE ignorée : FREQ invalide ou non prise en charge".to_string());
                }
            }
            "INTERVAL" => match raw.parse::<u32>() {
                Ok(value) if value >= 1 => interval = value,
                _ => warnings.push("RRULE : INTERVAL invalide ignoré".to_string()),
            },
            "COUNT" => match raw.parse::<u32>() {
                Ok(value) if value >= 1 => count = Some(value),
                _ => warnings.push("RRULE : COUNT invalide ignoré".to_string()),
            },
            "UNTIL" => match parse_until_value(raw) {
                Some(date) => until = Some(date),
                None => warnings.push("RRULE : UNTIL invalide ignoré".to_string()),
            },
            "BYDAY" => {
                for item in raw.split(',') {
                    let item = item.trim();
                    if item.is_empty() {
                        continue;
                    }
                    match parse_by_day_item(item) {
                        Some(day) => {
                            if !by_day.contains(&day) {
                                by_day.push(day);
                            }
                        }
                        None => warnings.push("RRULE : BYDAY invalide ignoré".to_string()),
                    }
                }
            }
            "BYMONTHDAY" => {
                for item in raw.split(',') {
                    let item = item.trim();
                    if item.is_empty() {
                        continue;
                    }
                    match item.parse::<i32>() {
                        Ok(value) if (1..=31).contains(&value) || (-31..=-1).contains(&value) => {
                            if !by_month_day.contains(&value) {
                                by_month_day.push(value);
                            }
                        }
                        _ => warnings.push("RRULE : BYMONTHDAY invalide ignoré".to_string()),
                    }
                }
            }
            "BYMONTH" => {
                for item in raw.split(',') {
                    let item = item.trim();
                    if item.is_empty() {
                        continue;
                    }
                    match item.parse::<u32>() {
                        Ok(value) if (1..=12).contains(&value) => {
                            if !by_month.contains(&value) {
                                by_month.push(value);
                            }
                        }
                        _ => warnings.push("RRULE : BYMONTH invalide ignoré".to_string()),
                    }
                }
            }
            "WKST" => match parse_weekday(raw) {
                Some(day) => wkst = day,
                None => warnings.push("RRULE : WKST invalide ignoré".to_string()),
            },
            _ => {}
        }
    }

    let freq = freq?;
    Some(Recurrence {
        freq,
        interval,
        count,
        until,
        by_day,
        by_month_day,
        by_month,
        wkst,
    })
}

// ---------------------------------------------------------------------------
// Recurrence expansion
// ---------------------------------------------------------------------------

const MAX_ITERATIONS: usize = 20_000;
const MAX_GENERATED: usize = 10_000;

struct GenState<'a> {
    rec: &'a Recurrence,
    start: NaiveDate,
    to: NaiveDate,
    count: Option<u32>,
    emitted: u32,
    iterations: usize,
    out: &'a mut BTreeSet<NaiveDate>,
    stopped: bool,
}

impl GenState<'_> {
    fn fresh_iteration(&mut self) -> bool {
        self.iterations += 1;
        if self.iterations > MAX_ITERATIONS || self.out.len() >= MAX_GENERATED {
            self.stopped = true;
        }
        !self.stopped
    }

    fn offer(&mut self, date: NaiveDate) {
        if self.stopped || date <= self.start {
            return;
        }
        if date > self.to {
            self.stopped = true;
            return;
        }
        if let Some(until) = self.rec.until
            && date > until
        {
            self.stopped = true;
            return;
        }
        if !self.rec.by_month.is_empty() && !self.rec.by_month.contains(&date.month()) {
            return;
        }
        self.out.insert(date);
        self.emitted += 1;
        if let Some(limit) = self.count
            && self.emitted >= limit
        {
            self.stopped = true;
        }
        if self.out.len() >= MAX_GENERATED {
            self.stopped = true;
        }
    }
}

fn raw_occurrences(event: &Event, from: NaiveDate, to: NaiveDate) -> BTreeSet<NaiveDate> {
    let mut out = BTreeSet::new();
    out.insert(event.start);

    let Some(rec) = event.rrule.as_ref() else {
        return out;
    };
    let count = rec.count.map(|value| value.max(1));
    if count.is_some_and(|limit| limit <= 1) {
        return out;
    }
    if to <= event.start || rec.until.is_some_and(|until| until < event.start) {
        return out;
    }

    let span = event.span_days.max(1) as u64;
    let earliest = from.checked_sub_days(Days::new(span.saturating_sub(1)));

    {
        let mut state = GenState {
            rec,
            start: event.start,
            to,
            count,
            emitted: 1,
            iterations: 0,
            out: &mut out,
            stopped: false,
        };
        match rec.freq {
            Freq::Daily => gen_daily(&mut state, earliest),
            Freq::Weekly => gen_weekly(&mut state, earliest),
            Freq::Monthly => gen_monthly(&mut state, earliest),
            Freq::Yearly => gen_yearly(&mut state, earliest),
        }
    }
    out
}

fn gen_daily(state: &mut GenState, earliest: Option<NaiveDate>) {
    let step = state.rec.interval.max(1) as u64;
    let mut day = state.start;

    // Fast-forward towards the requested range. With COUNT and BYMONTH the
    // filtered dates would make the accounting awkward, so only the simple
    // cases (the ones that can actually grow large) are skipped.
    if let Some(target) = earliest
        && target > day
        && (state.count.is_none() || state.rec.by_month.is_empty())
    {
        let diff = (target - day).num_days().max(0) as u64;
        let periods = diff / step;
        if periods > 0 {
            let Some(advanced) = day.checked_add_days(Days::new(periods.saturating_mul(step)))
            else {
                return;
            };
            if let Some(limit) = state.count {
                // `advanced` is the last rule occurrence at or before the
                // target; dates strictly before it are skipped but counted.
                let skipped = periods.saturating_sub(1).min(u32::MAX as u64) as u32;
                if state.emitted.saturating_add(skipped) >= limit {
                    return;
                }
                state.emitted = state.emitted.saturating_add(skipped);
            }
            day = advanced;
        }
    }

    loop {
        if !state.fresh_iteration() || day > state.to {
            break;
        }
        if state.rec.until.is_some_and(|until| day > until) {
            break;
        }
        state.offer(day);
        if state.stopped {
            break;
        }
        let Some(next) = day.checked_add_days(Days::new(step)) else {
            break;
        };
        day = next;
    }
}

fn weekday_offset(wkst: Weekday, weekday: Weekday) -> u64 {
    let from = wkst.num_days_from_monday() as i64;
    let to = weekday.num_days_from_monday() as i64;
    (to - from).rem_euclid(7) as u64
}

fn week_start(date: NaiveDate, wkst: Weekday) -> Option<NaiveDate> {
    date.checked_sub_days(Days::new(weekday_offset(wkst, date.weekday())))
}

fn gen_weekly(state: &mut GenState, earliest: Option<NaiveDate>) {
    let interval = state.rec.interval.max(1) as u64;
    let wkst = state.rec.wkst;
    let Some(mut week) = week_start(state.start, wkst) else {
        return;
    };

    if state.count.is_none()
        && let Some(target) = earliest
        && target > week
    {
        let diff = (target - week).num_days().max(0) as u64;
        let periods = (diff / 7) / interval;
        if periods > 0
            && let Some(advanced) = week.checked_add_days(Days::new(
                periods.saturating_mul(7).saturating_mul(interval),
            ))
        {
            week = advanced;
        }
    }

    let weekdays: Vec<Weekday> = if state.rec.by_day.is_empty() {
        vec![state.start.weekday()]
    } else {
        let mut seen = HashSet::new();
        let mut weekdays = Vec::new();
        for by_day in &state.rec.by_day {
            if seen.insert(by_day.weekday) {
                weekdays.push(by_day.weekday);
            }
        }
        weekdays
    };

    loop {
        if !state.fresh_iteration() || week > state.to {
            break;
        }
        let mut dates = BTreeSet::new();
        for weekday in &weekdays {
            if let Some(date) = week.checked_add_days(Days::new(weekday_offset(wkst, *weekday))) {
                dates.insert(date);
            }
        }
        for date in dates {
            state.offer(date);
            if state.stopped {
                break;
            }
        }
        if state.stopped {
            break;
        }
        let Some(next) = week.checked_add_days(Days::new(7u64.saturating_mul(interval))) else {
            break;
        };
        week = next;
    }
}

fn month_index(year: i32, month: u32) -> i64 {
    year as i64 * 12 + month as i64 - 1
}

fn year_month_from_index(index: i64) -> Option<(i32, u32)> {
    let year = i32::try_from(index.div_euclid(12)).ok()?;
    let month = (index.rem_euclid(12) + 1) as u32;
    Some((year, month))
}

fn days_in_month(year: i32, month: u32) -> Option<u32> {
    let first = NaiveDate::from_ymd_opt(year, month, 1)?;
    let next = if month == 12 {
        NaiveDate::from_ymd_opt(year.checked_add(1)?, 1, 1)?
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)?
    };
    Some((next - first).num_days() as u32)
}

fn day_from_month_day(days_in_month: u32, month_day: i32) -> Option<u32> {
    if month_day > 0 {
        let day = month_day as u32;
        (day <= days_in_month).then_some(day)
    } else if month_day < 0 {
        let day = days_in_month as i32 + month_day + 1;
        (day >= 1).then_some(day as u32)
    } else {
        None
    }
}

/// Days of the month matching a BYDAY rule (`ordinal == 0` means every one).
fn weekday_days_in_month(year: i32, month: u32, weekday: Weekday, ordinal: i8) -> Vec<u32> {
    let Some(days_in_month) = days_in_month(year, month) else {
        return Vec::new();
    };
    let Some(first) = NaiveDate::from_ymd_opt(year, month, 1) else {
        return Vec::new();
    };
    let offset = weekday_offset(first.weekday(), weekday) as u32;
    let first_day = 1 + offset;
    if first_day > days_in_month {
        return Vec::new();
    }

    if ordinal == 0 {
        (first_day..=days_in_month).step_by(7).collect()
    } else if ordinal > 0 {
        let day = first_day + (ordinal as u32 - 1) * 7;
        if day <= days_in_month {
            vec![day]
        } else {
            Vec::new()
        }
    } else {
        let last = days_in_month - ((days_in_month - first_day) % 7);
        let occurrences = (last - first_day) / 7 + 1;
        let position = ordinal as i32 + occurrences as i32;
        if position >= 0 {
            let day = first_day + position as u32 * 7;
            if day <= days_in_month {
                vec![day]
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        }
    }
}

/// Dates in the whole year matching a BYDAY rule (`ordinal == 0` means every one).
fn weekday_dates_in_year(year: i32, weekday: Weekday, ordinal: i8) -> Vec<NaiveDate> {
    let Some(first_of_year) = NaiveDate::from_ymd_opt(year, 1, 1) else {
        return Vec::new();
    };
    let Some(last_of_year) = NaiveDate::from_ymd_opt(year, 12, 31) else {
        return Vec::new();
    };
    let first_offset = weekday_offset(first_of_year.weekday(), weekday);
    let Some(first) = first_of_year.checked_add_days(Days::new(first_offset)) else {
        return Vec::new();
    };

    if ordinal == 0 {
        let mut dates = Vec::new();
        let mut date = first;
        while date.year() == year {
            dates.push(date);
            let Some(next) = date.checked_add_days(Days::new(7)) else {
                break;
            };
            date = next;
        }
        dates
    } else if ordinal > 0 {
        let day = (ordinal as u64 - 1) * 7;
        match first.checked_add_days(Days::new(day)) {
            Some(date) if date.year() == year => vec![date],
            _ => Vec::new(),
        }
    } else {
        let last_offset = weekday_offset(last_of_year.weekday(), weekday);
        let Some(last) = last_of_year.checked_sub_days(Days::new(last_offset)) else {
            return Vec::new();
        };
        let steps = (-(ordinal as i64) - 1) as u64 * 7;
        match last.checked_sub_days(Days::new(steps)) {
            Some(date) if date.year() == year => vec![date],
            _ => Vec::new(),
        }
    }
}

fn gen_monthly(state: &mut GenState, earliest: Option<NaiveDate>) {
    let interval = state.rec.interval.max(1) as i64;
    let mut index = month_index(state.start.year(), state.start.month());

    if state.count.is_none()
        && let Some(target) = earliest
    {
        let target_index = month_index(target.year(), target.month());
        if target_index > index {
            let periods = (target_index - index) / interval;
            index = index.saturating_add(periods.saturating_mul(interval));
        }
    }

    loop {
        if !state.fresh_iteration() {
            break;
        }
        let Some((year, month)) = year_month_from_index(index) else {
            break;
        };
        let Some(month_start) = NaiveDate::from_ymd_opt(year, month, 1) else {
            break;
        };
        if month_start > state.to {
            break;
        }
        index = match index.checked_add(interval) {
            Some(next) => next,
            None => break,
        };
        if !state.rec.by_month.is_empty() && !state.rec.by_month.contains(&month) {
            continue;
        }
        let Some(days_in_month) = days_in_month(year, month) else {
            break;
        };

        let by_month_day = &state.rec.by_month_day;
        let by_day = &state.rec.by_day;
        let mut days: BTreeSet<u32> = BTreeSet::new();

        if !by_month_day.is_empty() {
            for month_day in by_month_day {
                if let Some(day) = day_from_month_day(days_in_month, *month_day) {
                    days.insert(day);
                }
            }
        }
        if !by_day.is_empty() {
            let mut weekday_days = BTreeSet::new();
            for rule in by_day {
                for day in weekday_days_in_month(year, month, rule.weekday, rule.ordinal) {
                    weekday_days.insert(day);
                }
            }
            if by_month_day.is_empty() {
                days = weekday_days;
            } else {
                days = days.intersection(&weekday_days).copied().collect();
            }
        }
        if by_month_day.is_empty() && by_day.is_empty() && state.start.day() <= days_in_month {
            days.insert(state.start.day());
        }

        for day in days {
            if let Some(date) = NaiveDate::from_ymd_opt(year, month, day) {
                state.offer(date);
                if state.stopped {
                    break;
                }
            }
        }
        if state.stopped {
            break;
        }
    }
}

fn gen_yearly(state: &mut GenState, earliest: Option<NaiveDate>) {
    let interval = state.rec.interval.max(1) as i64;
    let mut year = state.start.year() as i64;

    if state.count.is_none()
        && let Some(target) = earliest
    {
        let target_year = target.year() as i64;
        if target_year > year {
            let periods = (target_year - year) / interval;
            year = year.saturating_add(periods.saturating_mul(interval));
        }
    }

    loop {
        if !state.fresh_iteration() {
            break;
        }
        let Ok(year_i32) = i32::try_from(year) else {
            break;
        };
        let Some(year_start) = NaiveDate::from_ymd_opt(year_i32, 1, 1) else {
            break;
        };
        if year_start > state.to {
            break;
        }
        year = match year.checked_add(interval) {
            Some(next) => next,
            None => break,
        };

        let has_by_month = !state.rec.by_month.is_empty();
        let months: BTreeSet<u32> = if has_by_month {
            state.rec.by_month.iter().copied().collect()
        } else {
            [state.start.month()].into_iter().collect()
        };
        let mut dates: BTreeSet<NaiveDate> = BTreeSet::new();

        if !state.rec.by_month_day.is_empty() {
            for month in &months {
                let Some(days_in_month) = days_in_month(year_i32, *month) else {
                    continue;
                };
                for month_day in &state.rec.by_month_day {
                    if let Some(day) = day_from_month_day(days_in_month, *month_day)
                        && let Some(date) = NaiveDate::from_ymd_opt(year_i32, *month, day)
                    {
                        dates.insert(date);
                    }
                }
            }
        } else if !state.rec.by_day.is_empty() {
            if has_by_month {
                for month in &months {
                    for rule in &state.rec.by_day {
                        for day in
                            weekday_days_in_month(year_i32, *month, rule.weekday, rule.ordinal)
                        {
                            if let Some(date) = NaiveDate::from_ymd_opt(year_i32, *month, day) {
                                dates.insert(date);
                            }
                        }
                    }
                }
            } else {
                // Without BYMONTH, ordinals are relative to the whole year.
                for rule in &state.rec.by_day {
                    for date in weekday_dates_in_year(year_i32, rule.weekday, rule.ordinal) {
                        dates.insert(date);
                    }
                }
            }
        } else {
            for month in &months {
                if let Some(days_in_month) = days_in_month(year_i32, *month)
                    && state.start.day() <= days_in_month
                    && let Some(date) = NaiveDate::from_ymd_opt(year_i32, *month, state.start.day())
                {
                    dates.insert(date);
                }
            }
        }

        for date in dates {
            state.offer(date);
            if state.stopped {
                break;
            }
        }
        if state.stopped {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn calendar(body: &str) -> String {
        format!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//contcal//test//FR\r\n{body}END:VCALENDAR\r\n"
        )
    }

    fn event(body: &str) -> Event {
        let wrapped = format!("BEGIN:VEVENT\r\n{body}END:VEVENT\r\n");
        let parsed = parse_ics(&calendar(&wrapped));
        assert!(
            parsed.warnings.is_empty(),
            "unexpected warnings: {:?}",
            parsed.warnings
        );
        assert_eq!(parsed.events.len(), 1, "events: {:?}", parsed.events);
        parsed.events.into_iter().next().unwrap()
    }

    fn d(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    fn t(hour: u32, minute: u32, second: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(hour, minute, second).unwrap()
    }

    // 1.
    #[test]
    fn parses_folded_all_day_event_with_escaped_summary() {
        let input = "BEGIN:VCALENDAR\r\n\
                     BEGIN:VEVENT\r\n\
                     UID:abc-123\r\n\
                     DTSTART;VALUE=DATE:20240101\r\n\
                     DTEND;VALUE=DATE:20240104\r\n\
                     SUMMARY:Fête\\, anniversaire\\n2e ligne\r\n\
                     LOCATION:Chez Alice\r\n\
                     END:VEVENT\r\n\
                     END:VCALENDAR\r\n";
        let parsed = parse_ics(input);
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
        assert_eq!(parsed.events.len(), 1);

        let event = &parsed.events[0];
        assert_eq!(event.uid, "abc-123");
        assert_eq!(event.summary, "Fête, anniversaire\n2e ligne");
        assert_eq!(event.location.as_deref(), Some("Chez Alice"));
        assert_eq!(event.start, d(2024, 1, 1));
        assert_eq!(event.start_time, None);
        assert!(event.all_day);
        assert_eq!(event.span_days, 3);
        assert!(event.rrule.is_none());
        assert!(event.exdates.is_empty());
        assert!(event.rdates.is_empty());
        assert_eq!(event.recurrence_id, None);
    }

    // 2.
    #[test]
    fn parses_timed_events() {
        let utc_start = d(2024, 1, 15).and_hms_opt(12, 0, 0).unwrap();
        let expected = Local.from_utc_datetime(&utc_start);
        let utc = event(
            "UID:utc\r\nDTSTART:20240115T120000Z\r\nDTEND:20240115T140000Z\r\nSUMMARY:UTC\r\n",
        );
        assert!(!utc.all_day);
        assert_eq!(utc.start, expected.date_naive());
        assert_eq!(utc.start_time, Some(expected.time()));
        assert_eq!(utc.span_days, 1);

        let overnight = event(
            "UID:overnight\r\nDTSTART:20240101T230000\r\nDTEND:20240102T010000\r\nSUMMARY:Nuit\r\n",
        );
        assert!(!overnight.all_day);
        assert_eq!(overnight.start, d(2024, 1, 1));
        assert_eq!(overnight.start_time, Some(t(23, 0, 0)));
        assert_eq!(overnight.span_days, 2);

        let same_day = event(
            "UID:same\r\nDTSTART:20240101T090000\r\nDTEND:20240101T100000\r\nSUMMARY:Jour\r\n",
        );
        assert_eq!(same_day.span_days, 1);
    }

    // 3.
    #[test]
    fn parses_duration() {
        let days =
            event("UID:d1\r\nDTSTART;VALUE=DATE:20240101\r\nDURATION:P3D\r\nSUMMARY:3 jours\r\n");
        assert_eq!(days.span_days, 3);

        let weeks =
            event("UID:d2\r\nDTSTART;VALUE=DATE:20240101\r\nDURATION:P1W\r\nSUMMARY:1 semaine\r\n");
        assert_eq!(weeks.span_days, 7);

        let overnight =
            event("UID:d3\r\nDTSTART:20240101T230000\r\nDURATION:PT2H\r\nSUMMARY:Nuit\r\n");
        assert_eq!(overnight.span_days, 2);

        let same_day =
            event("UID:d4\r\nDTSTART:20240101T090000\r\nDURATION:PT1H30M\r\nSUMMARY:Matin\r\n");
        assert_eq!(same_day.span_days, 1);
    }

    // 4.
    #[test]
    fn handles_bom_folding_and_line_endings() {
        let input = "\u{feff}BEGIN:VCALENDAR\r\n\
                     BEGIN:VEVENT\r\n\
                     UID:fold\r\n\
                     SUMMARY:Très \r\n long\r\n\
                     DTSTART;VALUE=DATE:2024\r\n 0101\r\n\
                     END:VEVENT\r\n\
                     END:VCALENDAR\r\n";
        let parsed = parse_ics(input);
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
        assert_eq!(parsed.events.len(), 1);
        assert_eq!(parsed.events[0].summary, "Très long");
        assert_eq!(parsed.events[0].start, d(2024, 1, 1));

        // Lone CR line endings.
        let input = "BEGIN:VCALENDAR\rBEGIN:VEVENT\rUID:cr\rDTSTART;VALUE=DATE:20240202\rEND:VEVENT\rEND:VCALENDAR\r";
        let parsed = parse_ics(input);
        assert_eq!(parsed.events.len(), 1);
        assert_eq!(parsed.events[0].uid, "cr");
        assert_eq!(parsed.events[0].start, d(2024, 2, 2));
    }

    // 5.
    #[test]
    fn expands_weekly_byday_with_count_and_interval() {
        let ev = event(
            "UID:w1\r\nDTSTART;VALUE=DATE:20240101\r\n\
             RRULE:FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,WE,FR;COUNT=5\r\nSUMMARY:Hebdo\r\n",
        );
        let rule = ev.rrule.as_ref().unwrap();
        assert_eq!(rule.freq, Freq::Weekly);
        assert_eq!(rule.interval, 2);
        assert_eq!(rule.count, Some(5));
        assert_eq!(rule.wkst, Weekday::Mon);
        assert_eq!(rule.by_day.len(), 3);

        assert_eq!(
            occurrence_days(&ev, d(2024, 1, 1), d(2024, 2, 1)),
            vec![
                d(2024, 1, 1),
                d(2024, 1, 3),
                d(2024, 1, 5),
                d(2024, 1, 15),
                d(2024, 1, 17)
            ]
        );
    }

    // 6.
    #[test]
    fn expands_weekly_without_byday() {
        let ev = event(
            "UID:w2\r\nDTSTART;VALUE=DATE:20240103\r\n\
             RRULE:FREQ=WEEKLY;INTERVAL=2\r\nSUMMARY:Bihebdo\r\n",
        );
        assert_eq!(
            occurrence_days(&ev, d(2024, 1, 1), d(2024, 1, 31)),
            vec![d(2024, 1, 3), d(2024, 1, 17), d(2024, 1, 31)]
        );
    }

    #[test]
    fn weekly_honours_wkst() {
        let ev = event(
            "UID:wkst\r\nDTSTART;VALUE=DATE:20240101\r\n\
             RRULE:FREQ=WEEKLY;INTERVAL=2;WKST=SU;BYDAY=SU,MO;COUNT=5\r\nSUMMARY:Semaine\r\n",
        );
        assert_eq!(ev.rrule.as_ref().unwrap().wkst, Weekday::Sun);
        assert_eq!(
            occurrence_days(&ev, d(2023, 12, 25), d(2024, 2, 5)),
            vec![
                d(2024, 1, 1),
                d(2024, 1, 14),
                d(2024, 1, 15),
                d(2024, 1, 28),
                d(2024, 1, 29)
            ]
        );
    }

    // 7.
    #[test]
    fn expands_monthly_byday_ordinals() {
        let ev = event(
            "UID:m1\r\nDTSTART;VALUE=DATE:20240101\r\n\
             RRULE:FREQ=MONTHLY;COUNT=7;BYDAY=2TU,-1FR\r\nSUMMARY:Mensuel\r\n",
        );
        let rule = ev.rrule.as_ref().unwrap();
        assert_eq!(rule.freq, Freq::Monthly);
        assert_eq!(
            occurrence_days(&ev, d(2024, 1, 1), d(2024, 12, 31)),
            vec![
                d(2024, 1, 1),
                d(2024, 1, 9),
                d(2024, 1, 26),
                d(2024, 2, 13),
                d(2024, 2, 23),
                d(2024, 3, 12),
                d(2024, 3, 29)
            ]
        );

        let last_day = event(
            "UID:m2\r\nDTSTART;VALUE=DATE:20240131\r\n\
             RRULE:FREQ=MONTHLY;BYMONTHDAY=-1;COUNT=3\r\nSUMMARY:Fin de mois\r\n",
        );
        assert_eq!(
            occurrence_days(&last_day, d(2024, 1, 1), d(2024, 12, 31)),
            vec![d(2024, 1, 31), d(2024, 2, 29), d(2024, 3, 31)]
        );

        let missing_day = event(
            "UID:m3\r\nDTSTART;VALUE=DATE:20240130\r\n\
             RRULE:FREQ=MONTHLY;BYMONTHDAY=30;COUNT=3\r\nSUMMARY:Le 30\r\n",
        );
        assert_eq!(
            occurrence_days(&missing_day, d(2024, 1, 1), d(2024, 12, 31)),
            vec![d(2024, 1, 30), d(2024, 3, 30), d(2024, 4, 30)]
        );

        // BYMONTHDAY and BYDAY are intersected for MONTHLY rules.
        let friday_13th = event(
            "UID:m4\r\nDTSTART;VALUE=DATE:20240913\r\n\
             RRULE:FREQ=MONTHLY;BYDAY=FR;BYMONTHDAY=13;COUNT=3\r\nSUMMARY:Vendredi 13\r\n",
        );
        assert_eq!(
            occurrence_days(&friday_13th, d(2024, 1, 1), d(2025, 12, 31)),
            vec![d(2024, 9, 13), d(2024, 12, 13), d(2025, 6, 13)]
        );
    }

    // 8.
    #[test]
    fn expands_yearly_by_month_and_byday() {
        let thanksgiving = event(
            "UID:y1\r\nDTSTART;VALUE=DATE:20241128\r\n\
             RRULE:FREQ=YEARLY;BYMONTH=11;BYDAY=4TH;COUNT=3\r\nSUMMARY:Thanksgiving\r\n",
        );
        assert_eq!(
            occurrence_days(&thanksgiving, d(2024, 1, 1), d(2026, 12, 31)),
            vec![d(2024, 11, 28), d(2025, 11, 27), d(2026, 11, 26)]
        );

        let anniversary = event(
            "UID:y2\r\nDTSTART;VALUE=DATE:20200215\r\n\
             RRULE:FREQ=YEARLY;COUNT=3\r\nSUMMARY:Anniversaire\r\n",
        );
        assert_eq!(
            occurrence_days(&anniversary, d(2020, 1, 1), d(2022, 12, 31)),
            vec![d(2020, 2, 15), d(2021, 2, 15), d(2022, 2, 15)]
        );
    }

    // 9.
    #[test]
    fn until_is_inclusive() {
        let date_until = event(
            "UID:u1\r\nDTSTART;VALUE=DATE:20240101\r\n\
             RRULE:FREQ=DAILY;UNTIL=20240103\r\nSUMMARY:Jusqu'au 3\r\n",
        );
        assert_eq!(
            date_until.rrule.as_ref().unwrap().until,
            Some(d(2024, 1, 3))
        );
        assert_eq!(
            occurrence_days(&date_until, d(2024, 1, 1), d(2024, 1, 10)),
            vec![d(2024, 1, 1), d(2024, 1, 2), d(2024, 1, 3)]
        );

        let datetime_until = event(
            "UID:u2\r\nDTSTART;VALUE=DATE:20240102\r\n\
             RRULE:FREQ=DAILY;UNTIL=20240104T120000\r\nSUMMARY:Jusqu'au 4\r\n",
        );
        assert_eq!(
            occurrence_days(&datetime_until, d(2024, 1, 1), d(2024, 1, 10)),
            vec![d(2024, 1, 2), d(2024, 1, 3), d(2024, 1, 4)]
        );
    }

    // 10.
    #[test]
    fn exdate_removes_and_rdate_adds_after_count() {
        let ev = event(
            "UID:x1\r\nDTSTART;VALUE=DATE:20240101\r\n\
             RRULE:FREQ=DAILY;COUNT=5\r\n\
             EXDATE;TZID=Europe/Paris:20240102,20240103\r\n\
             EXDATE:20240104\r\n\
             RDATE:20231230,20240110\r\nSUMMARY:Exceptions\r\n",
        );
        assert_eq!(ev.exdates.len(), 3);
        assert!(ev.exdates.contains(&d(2024, 1, 2)));
        assert!(ev.exdates.contains(&d(2024, 1, 3)));
        assert!(ev.exdates.contains(&d(2024, 1, 4)));
        assert_eq!(ev.rdates, vec![d(2023, 12, 30), d(2024, 1, 10)]);

        // COUNT is applied before EXDATE: raw count is Jan 1..5, then 2,3,4 removed.
        assert_eq!(
            occurrence_days(&ev, d(2023, 12, 28), d(2024, 1, 12)),
            vec![
                d(2023, 12, 30),
                d(2024, 1, 1),
                d(2024, 1, 5),
                d(2024, 1, 10)
            ]
        );
    }

    #[test]
    fn exdate_utc_is_converted_to_local_date() {
        let utc = d(2024, 1, 10).and_hms_opt(12, 0, 0).unwrap();
        let local_date = Local.from_utc_datetime(&utc).date_naive();
        let ev = event(
            "UID:xz\r\nDTSTART;VALUE=DATE:20240101\r\n\
             RRULE:FREQ=DAILY;COUNT=20\r\n\
             EXDATE:20240110T120000Z\r\nSUMMARY:UTC\r\n",
        );
        assert!(ev.exdates.contains(&local_date));
        let got = occurrence_days(&ev, d(2024, 1, 1), d(2024, 1, 31));
        assert!(!got.contains(&local_date));
        assert_eq!(got.len(), 19);
    }

    // 11.
    #[test]
    fn occurrence_range_and_multiday_span() {
        let multi = event(
            "UID:multi\r\nDTSTART;VALUE=DATE:20240130\r\n\
             DTEND;VALUE=DATE:20240202\r\nSUMMARY:Multi\r\n",
        );
        assert_eq!(multi.span_days, 3);
        assert_eq!(
            occurrence_days(&multi, d(2024, 2, 1), d(2024, 2, 1)),
            vec![d(2024, 1, 30)]
        );
        assert_eq!(
            occurrence_days(&multi, d(2024, 1, 31), d(2024, 1, 31)),
            vec![d(2024, 1, 30)]
        );
        assert!(occurrence_days(&multi, d(2024, 2, 2), d(2024, 2, 2)).is_empty());
        assert!(occurrence_days(&multi, d(2024, 1, 1), d(2024, 1, 1)).is_empty());

        let overnight = event(
            "UID:nuit\r\nDTSTART:20240101T230000\r\n\
             DTEND:20240102T010000\r\nSUMMARY:Nuit\r\n",
        );
        assert_eq!(
            occurrence_days(&overnight, d(2024, 1, 2), d(2024, 1, 2)),
            vec![d(2024, 1, 1)]
        );
        assert!(occurrence_days(&overnight, d(2024, 1, 3), d(2024, 1, 3)).is_empty());

        // Weekly recurring two-day event: the start can be before `from`.
        let weekly = event(
            "UID:multi2\r\nDTSTART;VALUE=DATE:20240101\r\n\
             DTEND;VALUE=DATE:20240103\r\n\
             RRULE:FREQ=WEEKLY;COUNT=5\r\nSUMMARY:Deux jours\r\n",
        );
        assert_eq!(
            occurrence_days(&weekly, d(2024, 1, 9), d(2024, 1, 9)),
            vec![d(2024, 1, 8)]
        );
        assert!(occurrence_days(&weekly, d(2024, 1, 10), d(2024, 1, 10)).is_empty());
        assert_eq!(
            occurrence_days(&weekly, d(2024, 1, 10), d(2024, 1, 16)),
            vec![d(2024, 1, 15)]
        );
    }

    // 12.
    #[test]
    fn recurrence_id_override_suppresses_base_instance() {
        let input = calendar(
            "BEGIN:VEVENT\r\n\
             UID:series\r\n\
             DTSTART;VALUE=DATE:20240101\r\n\
             RRULE:FREQ=DAILY;COUNT=5\r\n\
             SUMMARY:Base\r\n\
             END:VEVENT\r\n\
             BEGIN:VEVENT\r\n\
             UID:series\r\n\
             RECURRENCE-ID;VALUE=DATE:20240103\r\n\
             DTSTART;VALUE=DATE:20240103\r\n\
             SUMMARY:Modifié\r\n\
             END:VEVENT\r\n",
        );
        let parsed = parse_ics(&input);
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
        assert_eq!(parsed.events.len(), 2);

        let base = parsed
            .events
            .iter()
            .find(|ev| ev.recurrence_id.is_none())
            .unwrap();
        let overridden = parsed
            .events
            .iter()
            .find(|ev| ev.recurrence_id.is_some())
            .unwrap();

        assert!(base.exdates.contains(&d(2024, 1, 3)));
        assert_eq!(
            occurrence_days(base, d(2024, 1, 1), d(2024, 1, 10)),
            vec![d(2024, 1, 1), d(2024, 1, 2), d(2024, 1, 4), d(2024, 1, 5)]
        );
        assert_eq!(overridden.recurrence_id, Some(d(2024, 1, 3)));
        assert_eq!(
            occurrence_days(overridden, d(2024, 1, 1), d(2024, 1, 10)),
            vec![d(2024, 1, 3)]
        );
    }

    // 13.
    #[test]
    fn skips_cancelled_events_and_warns_on_bad_vevents() {
        let input = calendar(
            "BEGIN:VEVENT\r\n\
             UID:cancelled\r\n\
             DTSTART;VALUE=DATE:20240101\r\n\
             STATUS:CANCELLED\r\n\
             SUMMARY:Annulé\r\n\
             END:VEVENT\r\n\
             BEGIN:VEVENT\r\n\
             UID:no-start\r\n\
             SUMMARY:Sans début\r\n\
             END:VEVENT\r\n\
             BEGIN:VEVENT\r\n\
             UID:no-start-2\r\n\
             SUMMARY:Sans début aussi\r\n\
             END:VEVENT\r\n\
             BEGIN:VEVENT\r\n\
             UID:bad-date\r\n\
             DTSTART;VALUE=DATE:20240230\r\n\
             SUMMARY:Date impossible\r\n\
             END:VEVENT\r\n\
             BEGIN:VEVENT\r\n\
             UID:ok\r\n\
             DTSTART:20240102T100000\r\n\
             SUMMARY:Valide\r\n\
             END:VEVENT\r\n",
        );
        let parsed = parse_ics(&input);
        assert_eq!(parsed.events.len(), 1);
        assert_eq!(parsed.events[0].uid, "ok");
        assert_eq!(parsed.warnings.len(), 2, "{:?}", parsed.warnings);
        assert!(
            parsed
                .warnings
                .iter()
                .any(|w| w.contains("DTSTART manquant")),
            "{:?}",
            parsed.warnings
        );
        assert!(
            parsed
                .warnings
                .iter()
                .any(|w| w.contains("DTSTART invalide")),
            "{:?}",
            parsed.warnings
        );

        assert!(looks_like_ics(&input));
        assert!(!looks_like_ics(
            "<html><body>BEGIN:VCALENDAR BEGIN:VEVENT</body></html>"
        ));
        assert!(looks_like_ics("BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n"));
        assert!(!looks_like_ics("pas un calendrier"));
        assert!(looks_like_ics(
            "\u{feff}  begin:vcalendar\r\nbegin:vevent\r\n"
        ));
    }

    // 14.
    #[test]
    fn expands_old_daily_event_quickly() {
        let ev = event(
            "UID:old\r\nDTSTART;VALUE=DATE:19900101\r\n\
             RRULE:FREQ=DAILY\r\nSUMMARY:Vieux\r\n",
        );
        let started = std::time::Instant::now();
        let days = occurrence_days(&ev, d(2026, 1, 1), d(2026, 1, 10));
        assert_eq!(days.len(), 10);
        assert_eq!(days.first(), Some(&d(2026, 1, 1)));
        assert_eq!(days.last(), Some(&d(2026, 1, 10)));
        assert!(
            started.elapsed().as_secs() < 5,
            "daily expansion took too long"
        );
    }

    #[test]
    fn counted_old_daily_event_fast_forwards() {
        let ev = event(
            "UID:old-count\r\nDTSTART;VALUE=DATE:19900101\r\n\
             RRULE:FREQ=DAILY;COUNT=20000\r\nSUMMARY:Vieux compté\r\n",
        );
        let started = std::time::Instant::now();
        assert_eq!(
            occurrence_days(&ev, d(2026, 1, 1), d(2026, 1, 3)),
            vec![d(2026, 1, 1), d(2026, 1, 2), d(2026, 1, 3)]
        );
        assert!(started.elapsed().as_secs() < 5);

        let exhausted = event(
            "UID:old-count-2\r\nDTSTART;VALUE=DATE:19900101\r\n\
             RRULE:FREQ=DAILY;COUNT=5000\r\nSUMMARY:Épuisé\r\n",
        );
        assert!(occurrence_days(&exhausted, d(2026, 1, 1), d(2026, 1, 3)).is_empty());
    }

    // Additional parser coverage.
    #[test]
    fn ignores_nested_components_and_unknown_properties() {
        let input = calendar(
            "BEGIN:VTIMEZONE\r\n\
             TZID:Europe/Paris\r\n\
             BEGIN:STANDARD\r\n\
             DTSTART:19701025T030000\r\n\
             END:STANDARD\r\n\
             END:VTIMEZONE\r\n\
             BEGIN:VEVENT\r\n\
             UID:nested\r\n\
             DTSTART;TZID=\"Europe/Paris\":20240102T090000\r\n\
             SUMMARY:Réunion\r\n\
             X-CUSTOM:ignoré\r\n\
             BEGIN:VALARM\r\n\
             ACTION:DISPLAY\r\n\
             SUMMARY:Alarme\r\n\
             TRIGGER:-PT15M\r\n\
             END:VALARM\r\n\
             END:VEVENT\r\n",
        );
        let parsed = parse_ics(&input);
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
        assert_eq!(parsed.events.len(), 1);
        let ev = &parsed.events[0];
        assert_eq!(ev.summary, "Réunion");
        assert_eq!(ev.start, d(2024, 1, 2));
        assert_eq!(ev.start_time, Some(t(9, 0, 0)));
    }

    #[test]
    fn accepts_parameters_in_any_order() {
        let ev = event(
            "UID:params\r\n\
             DTSTART;TZID=Europe/Paris;VALUE=DATE:20240105\r\n\
             SUMMARY:Paramètres\r\n",
        );
        assert_eq!(ev.start, d(2024, 1, 5));
        assert!(ev.all_day);
        assert_eq!(ev.start_time, None);
    }

    #[test]
    fn defaults_empty_summary_and_generates_uid() {
        let ev = event("DTSTART;VALUE=DATE:20240101\r\nSUMMARY:   \r\n");
        assert_eq!(ev.summary, "(sans titre)");
        assert!(ev.uid.starts_with("no-uid-"), "uid: {}", ev.uid);
    }

    #[test]
    fn strips_control_characters_but_keeps_newlines() {
        let ev = event(
            "UID:ctrl\r\nDTSTART;VALUE=DATE:20240101\r\n\
             SUMMARY:Un\ttab \u{1b}et\\nune ligne\r\n",
        );
        assert_eq!(ev.summary, "Untab et\nune ligne");
    }

    #[test]
    fn unsupported_frequency_keeps_single_event() {
        let parsed = parse_ics(&calendar(
            "BEGIN:VEVENT\r\nUID:hourly\r\nDTSTART;VALUE=DATE:20240101\r\n\
             RRULE:FREQ=HOURLY\r\nEND:VEVENT\r\n",
        ));
        assert_eq!(parsed.events.len(), 1);
        assert!(parsed.events[0].rrule.is_none());
        assert!(
            parsed.warnings.iter().any(|w| w.contains("FREQ")),
            "{:?}",
            parsed.warnings
        );
        assert_eq!(
            occurrence_days(&parsed.events[0], d(2024, 1, 1), d(2024, 1, 1)),
            vec![d(2024, 1, 1)]
        );
    }

    #[test]
    fn invalid_interval_defaults_to_one() {
        let parsed = parse_ics(&calendar(
            "BEGIN:VEVENT\r\nUID:interval\r\nDTSTART;VALUE=DATE:20240101\r\n\
             RRULE:FREQ=DAILY;INTERVAL=0;COUNT=2\r\nEND:VEVENT\r\n",
        ));
        assert_eq!(parsed.events.len(), 1);
        let rule = parsed.events[0].rrule.as_ref().unwrap();
        assert_eq!(rule.interval, 1);
        assert!(
            parsed.warnings.iter().any(|w| w.contains("INTERVAL")),
            "{:?}",
            parsed.warnings
        );
    }

    #[test]
    fn by_month_filters_daily_and_monthly_rules() {
        let daily = event(
            "UID:bm1\r\nDTSTART;VALUE=DATE:20240130\r\n\
             RRULE:FREQ=DAILY;COUNT=5;BYMONTH=2\r\nSUMMARY:Février\r\n",
        );
        assert_eq!(
            occurrence_days(&daily, d(2024, 1, 1), d(2024, 3, 1)),
            vec![
                d(2024, 1, 30),
                d(2024, 2, 1),
                d(2024, 2, 2),
                d(2024, 2, 3),
                d(2024, 2, 4)
            ]
        );

        let monthly = event(
            "UID:bm2\r\nDTSTART;VALUE=DATE:20240115\r\n\
             RRULE:FREQ=MONTHLY;INTERVAL=2;BYMONTH=3;COUNT=2\r\nSUMMARY:Mars\r\n",
        );
        assert_eq!(
            occurrence_days(&monthly, d(2024, 1, 1), d(2024, 12, 31)),
            vec![d(2024, 1, 15), d(2024, 3, 15)]
        );
    }

    #[test]
    fn malformed_input_does_not_panic() {
        let inputs = [
            "",
            "\u{feff}",
            "BEGIN:VCALENDAR\nBEGIN:VEVENT\nDTSTART:not-a-date\nEND:VEVENT\nEND:VCALENDAR",
            "BEGIN:VEVENT\r\nDTSTART;VALUE=DATE:99999999\r\nEND:VEVENT",
            "BEGIN:VEVENT\r\nDTSTART:20240101T999999\r\nEND:VEVENT",
            "BEGIN:VEVENT\r\nDTSTART:00000101T000000Z\r\nEND:VEVENT",
            "BEGIN:VEVENT\r\nDTSTART:99991231T235959Z\r\nEND:VEVENT",
            "BEGIN:VEVENT\r\nDTSTART;VALUE=DATE:20240101\r\n\
             RRULE:FREQ=DAILY;INTERVAL=;COUNT=999999999999999999999\r\nEND:VEVENT",
            "BEGIN:VEVENT\r\nDTSTART:20240101T000000\r\n\
             DURATION:P999999999999999999D\r\nEND:VEVENT",
            "BEGIN:VEVENT\r\nDTSTART;FOO=\"a;b:c\";VALUE=DATE:20240101\r\nEND:VEVENT",
            ":",
            "BEGIN:VEVENT\nEND:VEVENT",
            "BEGIN;X=\"a:b\"\nEND",
        ];
        for input in inputs {
            let parsed = parse_ics(input);
            for event in &parsed.events {
                let _ = occurrence_days(event, d(2024, 1, 1), d(2025, 1, 1));
            }
            let _ = looks_like_ics(input);
        }
    }
}
