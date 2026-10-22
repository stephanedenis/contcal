//! Petites chaînes localisées (français par défaut, anglais en option).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Fr,
    En,
}

impl Lang {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "fr" | "francais" | "français" | "french" => Some(Self::Fr),
            "en" | "english" | "anglais" => Some(Self::En),
            _ => None,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::Fr => "fr",
            Self::En => "en",
        }
    }

    /// Noms complets des mois, index 0 = janvier.
    pub fn months(self) -> [&'static str; 12] {
        match self {
            Self::Fr => [
                "janvier",
                "février",
                "mars",
                "avril",
                "mai",
                "juin",
                "juillet",
                "août",
                "septembre",
                "octobre",
                "novembre",
                "décembre",
            ],
            Self::En => [
                "January",
                "February",
                "March",
                "April",
                "May",
                "June",
                "July",
                "August",
                "September",
                "October",
                "November",
                "December",
            ],
        }
    }

    pub fn months_short(self) -> [&'static str; 12] {
        match self {
            Self::Fr => [
                "janv", "févr", "mars", "avr", "mai", "juin", "juil", "août", "sept", "oct", "nov",
                "déc",
            ],
            Self::En => [
                "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
            ],
        }
    }

    /// Jours de la semaine, index 0 = lundi.
    pub fn weekdays(self) -> [&'static str; 7] {
        match self {
            Self::Fr => [
                "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche",
            ],
            Self::En => [
                "Monday",
                "Tuesday",
                "Wednesday",
                "Thursday",
                "Friday",
                "Saturday",
                "Sunday",
            ],
        }
    }

    /// Abréviations de jours, index 0 = lundi.
    pub fn weekdays_short(self) -> [&'static str; 7] {
        match self {
            Self::Fr => ["lun", "mar", "mer", "jeu", "ven", "sam", "dim"],
            Self::En => ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"],
        }
    }

    pub fn date_short(self, d: chrono::NaiveDate) -> String {
        use chrono::Datelike;
        match self {
            Self::Fr => format!(
                "{} {} {}",
                self.weekdays_short()[d.weekday().num_days_from_monday() as usize],
                d.day(),
                self.months()[d.month0() as usize]
            ),
            Self::En => format!(
                "{}, {} {}",
                self.weekdays_short()[d.weekday().num_days_from_monday() as usize],
                self.months()[d.month0() as usize],
                d.day()
            ),
        }
    }

    pub fn week_label(self) -> &'static str {
        match self {
            Self::Fr => "Sem",
            Self::En => "Wk",
        }
    }

    pub fn week_prefix(self) -> &'static str {
        match self {
            Self::Fr => "S",
            Self::En => "W",
        }
    }

    pub fn t(self, key: &'static str) -> &'static str {
        match (self, key) {
            (Self::Fr, "today") => "aujourd'hui",
            (Self::En, "today") => "today",
            (Self::Fr, "events") => "événements",
            (Self::En, "events") => "events",
            (Self::Fr, "event") => "événement",
            (Self::En, "event") => "event",
            (Self::Fr, "all_day") => "journée",
            (Self::En, "all_day") => "all-day",
            (Self::Fr, "scroll") => "molette",
            (Self::En, "scroll") => "wheel",
            (Self::Fr, "quit") => "quitter",
            (Self::En, "quit") => "quit",
            (Self::Fr, "refresh") => "actualiser",
            (Self::En, "refresh") => "refresh",
            (Self::Fr, "today_help") => "aujourd'hui",
            (Self::En, "today_help") => "today",
            (Self::Fr, "config") => "config",
            (Self::En, "config") => "config",
            (Self::Fr, "help") => "aide",
            (Self::En, "help") => "help",
            (Self::Fr, "info") => "informations",
            (Self::En, "info") => "information",
            (Self::Fr, "close") => "Échap pour fermer",
            (Self::En, "close") => "Esc to close",
            (Self::Fr, "calendar") => "Calendrier",
            (Self::En, "calendar") => "Calendar",
            (Self::Fr, "calendars") => "Calendriers distants",
            (Self::En, "calendars") => "Remote calendars",
            (Self::Fr, "shortcuts") => "Raccourcis",
            (Self::En, "shortcuts") => "Shortcuts",
            (Self::Fr, "configuration") => "Configuration",
            (Self::En, "configuration") => "Configuration",
            (Self::Fr, "terminal") => "Terminal",
            (Self::En, "terminal") => "Terminal",
            (Self::Fr, "view") => "Vue",
            (Self::En, "view") => "View",
            (Self::Fr, "theme") => "Thème",
            (Self::En, "theme") => "Theme",
            (Self::Fr, "dark") => "sombre",
            (Self::En, "dark") => "dark",
            (Self::Fr, "light") => "clair",
            (Self::En, "light") => "light",
            (Self::Fr, "disabled") => "désactivé",
            (Self::En, "disabled") => "disabled",
            (Self::Fr, "url_missing") => "URL manquante",
            (Self::En, "url_missing") => "missing URL",
            (Self::Fr, "loading") => "chargement…",
            (Self::En, "loading") => "loading…",
            (Self::Fr, "never") => "jamais",
            (Self::En, "never") => "never",
            (Self::Fr, "error") => "erreur",
            (Self::En, "error") => "error",
            (Self::Fr, "no_remote") => {
                "Aucun calendrier distant actif : renseignez ou activez les URL ICS dans la configuration."
            }
            (Self::En, "no_remote") => {
                "No active remote calendar: fill in or enable ICS URLs in the configuration."
            }
            (Self::Fr, "created") => "Configuration créée",
            (Self::En, "created") => "Configuration created",
            (Self::Fr, "reloaded") => "Configuration rechargée",
            (Self::En, "reloaded") => "Configuration reloaded",
            (Self::Fr, "reload_error") => "Configuration invalide, ancienne version conservée",
            (Self::En, "reload_error") => "Invalid configuration, keeping previous one",
            (Self::Fr, "fetch_ok") => "actualisé",
            (Self::En, "fetch_ok") => "updated",
            (Self::Fr, "no_events") => "aucun événement",
            (Self::En, "no_events") => "no events",
            (Self::Fr, "more") => "de plus",
            (Self::En, "more") => "more",
            (Self::Fr, "week") => "semaine",
            (Self::En, "week") => "week",
            (Self::Fr, "weeks") => "semaines",
            (Self::En, "weeks") => "weeks",
            (Self::Fr, "row") => "ligne",
            (Self::En, "row") => "row",
            (Self::Fr, "rows") => "lignes",
            (Self::En, "rows") => "rows",
            (Self::Fr, "min_cols") => "Terminal trop étroit (28 colonnes minimum).",
            (Self::En, "min_cols") => "Terminal too narrow (28 columns minimum).",
            (Self::Fr, "min_rows") => "Terminal trop petit (6 lignes minimum).",
            (Self::En, "min_rows") => "Terminal too small (6 rows minimum).",
            (Self::Fr, "month_color") => "couleur du mois",
            (Self::En, "month_color") => "month colour",
            (Self::Fr, "tint") => "teinte",
            (Self::En, "tint") => "tint",
            (Self::Fr, "line") => "ligne",
            (Self::En, "line") => "line",
            (Self::Fr, "lines") => "lignes",
            (Self::En, "lines") => "lines",
            (_, _) => "",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn french_names() {
        let lang = Lang::Fr;
        assert_eq!(lang.months()[0], "janvier");
        assert_eq!(lang.months_short()[6], "juil");
        assert_eq!(lang.weekdays_short()[0], "lun");
        let d = NaiveDate::from_ymd_opt(2026, 7, 6).unwrap();
        assert_eq!(lang.date_short(d), "lun 6 juillet");
    }

    #[test]
    fn parses_codes() {
        assert_eq!(Lang::parse("FR"), Some(Lang::Fr));
        assert_eq!(Lang::parse("english"), Some(Lang::En));
        assert_eq!(Lang::parse("de"), None);
    }
}
