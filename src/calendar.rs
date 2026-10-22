//! Dates du calendrier continu : semaines, mois, navigation.

use chrono::{Datelike, Duration, Months, NaiveDate, Weekday};

use crate::config::WeekStart;
use crate::lang::Lang;

/// Bornes raisonnables pour un défilement « infini » sans casser le formatage.
pub fn min_anchor() -> NaiveDate {
    NaiveDate::from_ymd_opt(1900, 1, 1).unwrap_or(NaiveDate::MIN)
}

pub fn max_anchor() -> NaiveDate {
    NaiveDate::from_ymd_opt(2200, 12, 31).unwrap_or(NaiveDate::MAX)
}

pub fn clamp_anchor(date: NaiveDate) -> NaiveDate {
    date.max(min_anchor()).min(max_anchor())
}

/// Lundi (ou dimanche) de la semaine contenant `date`.
pub fn snap_week_start(date: NaiveDate, week_start: WeekStart) -> NaiveDate {
    let offset = match week_start {
        WeekStart::Monday => date.weekday().num_days_from_monday() as i64,
        WeekStart::Sunday => date.weekday().num_days_from_sunday() as i64,
    };
    clamp_anchor(date - Duration::days(offset))
}

/// Index du premier jour de la semaine dans la liste lundi→dimanche.
pub fn start_index(week_start: WeekStart) -> usize {
    match week_start {
        WeekStart::Monday => 0,
        WeekStart::Sunday => 6,
    }
}

/// Les 7 jours d'une rangée (l'ancre doit déjà être alignée).
pub fn row_days(anchor: NaiveDate) -> [NaiveDate; 7] {
    std::array::from_fn(|i| anchor + Duration::days(i as i64))
}

pub fn is_weekend(date: NaiveDate) -> bool {
    matches!(date.weekday(), Weekday::Sat | Weekday::Sun)
}

/// Numéro de semaine ISO de la rangée (calculé sur son jeudi, stable).
pub fn week_number(anchor: NaiveDate) -> u32 {
    (anchor + Duration::days(3)).iso_week().week()
}

/// Premier jour du mois s'il tombe dans cette rangée.
pub fn first_of_month_in_row(anchor: NaiveDate) -> Option<NaiveDate> {
    row_days(anchor).into_iter().find(|d| d.day() == 1)
}

pub fn add_weeks(anchor: NaiveDate, delta: i64) -> NaiveDate {
    let days = delta.saturating_mul(7);
    anchor
        .checked_add_signed(Duration::days(days))
        .map(clamp_anchor)
        .unwrap_or_else(|| {
            if delta < 0 {
                min_anchor()
            } else {
                max_anchor()
            }
        })
}

/// Avance ou recule d'un mois, puis réaligne sur le début de semaine.
pub fn add_months(anchor: NaiveDate, delta: i32, week_start: WeekStart) -> NaiveDate {
    let target = if delta >= 0 {
        anchor.checked_add_months(Months::new(delta.unsigned_abs()))
    } else {
        anchor.checked_sub_months(Months::new(delta.unsigned_abs()))
    };
    match target {
        Some(d) => snap_week_start(clamp_anchor(d), week_start),
        None => anchor,
    }
}

/// Place la semaine de `target` au milieu des `weeks` rangées visibles.
pub fn centered_anchor(target: NaiveDate, weeks: u16, week_start: WeekStart) -> NaiveDate {
    let base = snap_week_start(target, week_start);
    let offset = i64::from(weeks / 2) * 7;
    base.checked_sub_signed(Duration::days(offset))
        .map(clamp_anchor)
        .unwrap_or(base)
}

pub fn today_anchor(today: NaiveDate, weeks: u16, week_start: WeekStart) -> NaiveDate {
    centered_anchor(today, weeks, week_start)
}

pub fn last_visible(anchor: NaiveDate, weeks: u16) -> NaiveDate {
    anchor + Duration::days(i64::from(weeks) * 7 - 1)
}

pub fn month_title(lang: Lang, month: u32, year: i32) -> String {
    let name = lang
        .months()
        .get(month.saturating_sub(1) as usize)
        .copied()
        .unwrap_or("");
    format!("{name} {year}")
}

/// « juillet 2026 », « juil – oct 2026 » ou « déc 2026 – févr 2027 ».
pub fn range_label(lang: Lang, first: NaiveDate, last: NaiveDate) -> String {
    let (m1, y1) = (first.month(), first.year());
    let (m2, y2) = (last.month(), last.year());
    if m1 == m2 && y1 == y2 {
        month_title(lang, m1, y1)
    } else if y1 == y2 {
        let a = lang.months_short()[m1 as usize - 1];
        let b = lang.months_short()[m2 as usize - 1];
        format!("{a} – {b} {y1}")
    } else {
        let a = lang.months_short()[m1 as usize - 1];
        let b = lang.months_short()[m2 as usize - 1];
        format!("{a} {y1} – {b} {y2}")
    }
}

/// « 3 semaines », « 1 semaine ».
pub fn weeks_phrase(lang: Lang, n: u64) -> String {
    match lang {
        Lang::Fr => {
            if n <= 1 {
                format!("{n} semaine")
            } else {
                format!("{n} semaines")
            }
        }
        Lang::En => {
            if n == 1 {
                format!("{n} week")
            } else {
                format!("{n} weeks")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn snaps_monday_and_sunday() {
        let wed = d(2026, 7, 8);
        assert_eq!(snap_week_start(wed, WeekStart::Monday), d(2026, 7, 6));
        assert_eq!(snap_week_start(wed, WeekStart::Sunday), d(2026, 7, 5));
        let sunday = d(2026, 7, 5);
        assert_eq!(snap_week_start(sunday, WeekStart::Monday), d(2026, 6, 29));
        assert_eq!(snap_week_start(sunday, WeekStart::Sunday), d(2026, 7, 5));
    }

    #[test]
    fn rows_start_on_configured_day() {
        let anchor = d(2026, 7, 6);
        let row = row_days(anchor);
        assert_eq!(row[0], anchor);
        assert_eq!(row[6], d(2026, 7, 12));
        assert_eq!(row[0].weekday(), Weekday::Mon);
        assert!(is_weekend(row[5]) && is_weekend(row[6]));
        assert_eq!(week_number(anchor), 28);
    }

    #[test]
    fn finds_first_of_month() {
        assert_eq!(first_of_month_in_row(d(2026, 6, 29)), Some(d(2026, 7, 1)));
        assert_eq!(first_of_month_in_row(d(2026, 7, 6)), None);
    }

    #[test]
    fn month_navigation_realigns() {
        let a = d(2026, 7, 6);
        assert_eq!(add_months(a, 1, WeekStart::Monday), d(2026, 8, 3));
        assert_eq!(add_months(a, -1, WeekStart::Monday), d(2026, 6, 1));
        // 31 janvier + 1 mois : chrono ramène au 28 février, puis lundi.
        let jan31 = d(2026, 1, 31);
        assert_eq!(add_months(jan31, 1, WeekStart::Monday), d(2026, 2, 23));
    }

    #[test]
    fn centers_target_week() {
        let today = d(2026, 7, 8);
        let anchor = centered_anchor(today, 10, WeekStart::Monday);
        assert_eq!(anchor, d(2026, 6, 1));
        let row_index = (today - anchor).num_days() / 7;
        assert_eq!(row_index, 5);
    }

    #[test]
    fn range_labels() {
        assert_eq!(
            range_label(Lang::Fr, d(2026, 7, 1), d(2026, 7, 31)),
            "juillet 2026"
        );
        assert_eq!(
            range_label(Lang::Fr, d(2026, 7, 6), d(2026, 10, 4)),
            "juil – oct 2026"
        );
        assert_eq!(
            range_label(Lang::Fr, d(2026, 12, 1), d(2027, 2, 1)),
            "déc 2026 – févr 2027"
        );
    }

    #[test]
    fn weeks_are_clamped_to_sane_bounds() {
        assert_eq!(add_weeks(max_anchor(), 100), max_anchor());
        assert_eq!(add_weeks(min_anchor(), -100), min_anchor());
        assert_eq!(add_weeks(d(2026, 7, 6), 1), d(2026, 7, 13));
    }
}
