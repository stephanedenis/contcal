//! Chargement, validation et valeurs par défaut de la configuration.
//!
//! Le fichier vit dans le home de l'utilisateur : `~/.contcal.toml`.
//! Il peut être remplacé par `$CONTCAL_CONFIG` ou l'option `--config`.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::NaiveDate;
use serde::Deserialize;

use crate::color::{DEFAULT_PALETTE, Rgb};
use crate::lang::Lang;

pub const CONFIG_FILENAME: &str = ".contcal.toml";

pub const DEFAULT_CONFIG: &str = r##"# ────────────────────────────────────────────────────────────────
#  contcal — calendrier continu pour le terminal
#  Fichier créé automatiquement dans le home (~/.contcal.toml).
#  Raccourci « c » : recharger · « i » : informations · « q » : quitter
# ────────────────────────────────────────────────────────────────

[calendar]
# Premier jour de la semaine : "monday" (lundi) ou "sunday" (dimanche).
week_start = "monday"

# Position d'ouverture : "today", ou une date "AAAA-MM-JJ".
start = "today"

# Numéros de semaine ISO dans la gouttière (et nom du mois à son début).
week_numbers = true

# Langue de l'interface : "fr" ou "en".
language = "fr"

[appearance]
# Hauteur d'une semaine en lignes : 1 = une semaine par rangée (compact),
# 2 = une ligne d'événements en plus, 3 à 8 pour davantage de détails.
row_height = 1

# Semaines parcourues par cran de molette.
scroll_weeks = 1

# Teinte de fond du mois en pourcentage (0 à 45). 0 = pas de fond coloré.
month_tint = 16

# Couleurs : "auto", "truecolor", "256" ou "mono".
color_mode = "auto"

# Fond supposé du terminal : "auto", "dark" ou "light".
background = "auto"

# Afficher les événements des calendriers distants.
show_events = true

# Préfixer les événements minutés par leur heure de début (si row_height > 1).
show_event_time = true

# Atténuer les samedis et dimanches.
weekend_dim = true

[events]
# Rafraîchissement automatique des flux ICS, en minutes (0 = jamais).
refresh_minutes = 30

# Délai maximum d'un téléchargement, en secondes.
timeout_seconds = 20

[colors]
# Couleur du jour courant.
today = "#f5c542"

# Palette des 12 mois (janvier → décembre), facultative.
# Décommentez et adaptez si vous le souhaitez.
# palette = [
#   "#6ea8fe", "#b197fc", "#4dd4c6", "#74c69d",
#   "#b5e48c", "#f4b860", "#ff9f68", "#ef6f6c",
#   "#d0a2ff", "#ffc857", "#63c7b2", "#93b7ff",
# ]

# ────────────────────────────────────────────────────────────────
#  Calendriers distants (flux iCalendar / ICS)
#
#  Outlook / Microsoft 365 :
#    Outlook web → Calendrier → Partager → Publier un calendrier
#    puis copiez le lien .ics (souvent .../calendar.ics).
#
#  Google Calendar :
#    Paramètres du calendrier → Intégrer le calendrier → Adresse secrète
#    au format iCal (…/basic.ics).
#
#  Les liens « webcal:// » sont acceptés. Ces URL contiennent souvent un
#  jeton secret : gardez ce fichier privé.
# ────────────────────────────────────────────────────────────────

[calendars.outlook]
enabled = true
name = "Outlook"
url = ""
# url = "https://outlook.office365.com/owa/calendar/<id>/<jeton>/calendar.ics"
# color = "#4ea1ff"

[calendars.google]
enabled = true
name = "Google"
url = ""
# url = "https://calendar.google.com/calendar/ical/<id>/public/basic.ics"
# color = "#ff6b6b"
"##;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub calendar: CalendarSection,
    #[serde(default)]
    pub appearance: AppearanceSection,
    #[serde(default)]
    pub events: EventsSection,
    #[serde(default)]
    pub colors: ColorsSection,
    #[serde(default)]
    pub calendars: BTreeMap<String, SourceConfig>,
}

impl Default for Config {
    fn default() -> Self {
        // Construit directement les valeurs : surtout ne pas re-parser le modèle
        // ici, serde pourrait rappeler `Config::default()` pour un champ absent.
        let mut calendars = BTreeMap::new();
        calendars.insert(
            "google".to_string(),
            SourceConfig {
                enabled: true,
                name: "Google".into(),
                url: String::new(),
                color: None,
            },
        );
        calendars.insert(
            "outlook".to_string(),
            SourceConfig {
                enabled: true,
                name: "Outlook".into(),
                url: String::new(),
                color: None,
            },
        );
        Self {
            calendar: CalendarSection::default(),
            appearance: AppearanceSection::default(),
            events: EventsSection::default(),
            colors: ColorsSection::default(),
            calendars,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct CalendarSection {
    pub week_start: String,
    pub start: String,
    pub week_numbers: bool,
    pub language: String,
}

impl Default for CalendarSection {
    fn default() -> Self {
        Self {
            week_start: "monday".into(),
            start: "today".into(),
            week_numbers: true,
            language: "fr".into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AppearanceSection {
    pub row_height: i64,
    pub scroll_weeks: i64,
    pub month_tint: i64,
    pub color_mode: String,
    pub background: String,
    pub show_events: bool,
    pub show_event_time: bool,
    pub weekend_dim: bool,
}

impl Default for AppearanceSection {
    fn default() -> Self {
        Self {
            row_height: 1,
            scroll_weeks: 1,
            month_tint: 16,
            color_mode: "auto".into(),
            background: "auto".into(),
            show_events: true,
            show_event_time: true,
            weekend_dim: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct EventsSection {
    pub refresh_minutes: i64,
    pub timeout_seconds: i64,
}

impl Default for EventsSection {
    fn default() -> Self {
        Self {
            refresh_minutes: 30,
            timeout_seconds: 20,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ColorsSection {
    pub today: String,
    pub palette: Vec<String>,
}

impl Default for ColorsSection {
    fn default() -> Self {
        Self {
            today: "#f5c542".into(),
            palette: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SourceConfig {
    pub enabled: bool,
    pub name: String,
    pub url: String,
    pub color: Option<String>,
}

impl Default for SourceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            name: String::new(),
            url: String::new(),
            color: None,
        }
    }
}

/// Premier jour d'une rangée.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeekStart {
    Monday,
    Sunday,
}

/// Point de départ de la vue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartPosition {
    Today,
    On(NaiveDate),
}

impl StartPosition {
    pub fn resolve(self, today: NaiveDate) -> NaiveDate {
        match self {
            Self::Today => today,
            Self::On(d) => d,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorChoice {
    Auto,
    TrueColor,
    Ansi256,
    Mono,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundChoice {
    Auto,
    Dark,
    Light,
}

#[derive(Debug, Clone)]
pub struct Source {
    pub key: String,
    pub name: String,
    pub url: String,
    pub enabled: bool,
    pub color: Rgb,
}

impl Source {
    /// Une source est réellement interrogée si elle est activée et pourvue d'une URL.
    pub fn is_active(&self) -> bool {
        self.enabled && !self.url.trim().is_empty()
    }
}

/// Configuration validée, prête pour l'interface.
#[derive(Debug, Clone)]
pub struct Settings {
    pub week_start: WeekStart,
    pub lang: Lang,
    pub start: StartPosition,
    pub week_numbers: bool,
    pub row_height: u16,
    pub scroll_weeks: i64,
    pub month_tint: u8,
    pub color_choice: ColorChoice,
    pub background_choice: BackgroundChoice,
    pub show_events: bool,
    pub show_event_time: bool,
    pub weekend_dim: bool,
    pub today_color: Rgb,
    pub palette: [Rgb; 12],
    /// `None` quand le rafraîchissement automatique est désactivé.
    pub refresh: Option<Duration>,
    pub timeout: Duration,
    pub sources: Vec<Source>,
    pub config_path: PathBuf,
}

impl Settings {
    pub fn from_config(config: &Config, config_path: PathBuf) -> Self {
        let week_start = match config.calendar.week_start.as_str() {
            "sunday" => WeekStart::Sunday,
            _ => WeekStart::Monday,
        };
        let lang = Lang::parse(&config.calendar.language).unwrap_or(Lang::Fr);
        let start = if config.calendar.start == "today" {
            StartPosition::Today
        } else {
            NaiveDate::parse_from_str(&config.calendar.start, "%Y-%m-%d")
                .map(StartPosition::On)
                .unwrap_or(StartPosition::Today)
        };
        let color_choice = match config.appearance.color_mode.as_str() {
            "truecolor" => ColorChoice::TrueColor,
            "256" => ColorChoice::Ansi256,
            "mono" => ColorChoice::Mono,
            _ => ColorChoice::Auto,
        };
        let background_choice = match config.appearance.background.as_str() {
            "dark" => BackgroundChoice::Dark,
            "light" => BackgroundChoice::Light,
            _ => BackgroundChoice::Auto,
        };
        let mut palette = DEFAULT_PALETTE;
        if config.colors.palette.len() == 12
            && let Some(parsed) = config
                .colors
                .palette
                .iter()
                .map(|s| Rgb::parse_hex(s))
                .collect::<Option<Vec<_>>>()
            && let Ok(arr) = <[Rgb; 12]>::try_from(parsed)
        {
            palette = arr;
        }
        let sources = config
            .calendars
            .iter()
            .filter(|(key, _)| !key.trim().is_empty())
            .map(|(key, src)| {
                let name = if src.name.trim().is_empty() {
                    match key.to_ascii_lowercase().as_str() {
                        "outlook" => "Outlook".to_string(),
                        "google" => "Google".to_string(),
                        _ => key.clone(),
                    }
                } else {
                    src.name.trim().to_string()
                };
                let color = src
                    .color
                    .as_deref()
                    .and_then(Rgb::parse_hex)
                    .unwrap_or_else(|| default_source_color(key));
                Source {
                    key: key.clone(),
                    name,
                    url: src.url.trim().to_string(),
                    enabled: src.enabled,
                    color,
                }
            })
            .collect();

        Self {
            week_start,
            lang,
            start,
            week_numbers: config.calendar.week_numbers,
            row_height: config.appearance.row_height.clamp(1, 8) as u16,
            scroll_weeks: config.appearance.scroll_weeks.clamp(1, 50),
            month_tint: config.appearance.month_tint.clamp(0, 45) as u8,
            color_choice,
            background_choice,
            show_events: config.appearance.show_events,
            show_event_time: config.appearance.show_event_time,
            weekend_dim: config.appearance.weekend_dim,
            today_color: Rgb::parse_hex(&config.colors.today)
                .unwrap_or_else(|| Rgb::new(0xf5, 0xc5, 0x42)),
            palette,
            refresh: (config.events.refresh_minutes > 0)
                .then(|| Duration::from_secs(config.events.refresh_minutes as u64 * 60)),
            timeout: Duration::from_secs(config.events.timeout_seconds.clamp(1, 120) as u64),
            sources,
            config_path,
        }
    }
}

fn default_source_color(key: &str) -> Rgb {
    match key.to_ascii_lowercase().as_str() {
        "outlook" => Rgb::new(0x4e, 0xa1, 0xff),
        "google" => Rgb::new(0xff, 0x6b, 0x6b),
        _ => Rgb::new(0x89, 0xb4, 0xfa),
    }
}

impl Config {
    /// Corrige les valeurs hors limites et renvoie des avertissements lisibles.
    pub fn validate(&mut self) -> Vec<String> {
        let mut warnings = Vec::new();

        let ws = self.calendar.week_start.trim().to_ascii_lowercase();
        self.calendar.week_start = match ws.as_str() {
            "monday" | "mon" | "lundi" | "lun" => "monday",
            "sunday" | "sun" | "dimanche" | "dim" => "sunday",
            _ => {
                warnings.push(format!(
                    "calendar.week_start « {} » inconnu : « monday » utilisé.",
                    self.calendar.week_start.trim()
                ));
                "monday"
            }
        }
        .to_string();

        let raw_start = self.calendar.start.trim().to_string();
        if raw_start.is_empty()
            || raw_start.eq_ignore_ascii_case("today")
            || raw_start.eq_ignore_ascii_case("aujourd'hui")
            || raw_start.eq_ignore_ascii_case("now")
        {
            self.calendar.start = "today".into();
        } else {
            let parsed = NaiveDate::parse_from_str(&raw_start, "%Y-%m-%d")
                .or_else(|_| NaiveDate::parse_from_str(&raw_start, "%d/%m/%Y"));
            match parsed {
                Ok(d) => self.calendar.start = d.format("%Y-%m-%d").to_string(),
                Err(_) => {
                    warnings.push(format!(
                        "calendar.start « {raw_start} » invalide (attendu : today ou AAAA-MM-JJ)."
                    ));
                    self.calendar.start = "today".into();
                }
            }
        }

        match Lang::parse(&self.calendar.language) {
            Some(lang) => self.calendar.language = lang.code().to_string(),
            None => {
                warnings.push(format!(
                    "calendar.language « {} » inconnu : « fr » utilisé.",
                    self.calendar.language
                ));
                self.calendar.language = "fr".into();
            }
        }

        self.appearance.row_height = clamp_with_warning(
            self.appearance.row_height,
            1,
            8,
            "appearance.row_height",
            &mut warnings,
        );
        self.appearance.scroll_weeks = clamp_with_warning(
            self.appearance.scroll_weeks,
            1,
            50,
            "appearance.scroll_weeks",
            &mut warnings,
        );
        self.appearance.month_tint = clamp_with_warning(
            self.appearance.month_tint,
            0,
            45,
            "appearance.month_tint",
            &mut warnings,
        );

        let mode = self.appearance.color_mode.trim().to_ascii_lowercase();
        self.appearance.color_mode = match mode.as_str() {
            "auto" => "auto",
            "truecolor" | "true-color" | "24bit" | "24-bit" | "rgb" => "truecolor",
            "256" | "ansi256" | "ansi-256" | "256color" => "256",
            "mono" | "none" | "no-color" | "nocolor" => "mono",
            _ => {
                warnings.push(format!(
                    "appearance.color_mode « {} » inconnu : « auto » utilisé.",
                    self.appearance.color_mode
                ));
                "auto"
            }
        }
        .to_string();

        let bg = self.appearance.background.trim().to_ascii_lowercase();
        self.appearance.background = match bg.as_str() {
            "auto" => "auto",
            "dark" | "sombre" => "dark",
            "light" | "clair" => "light",
            _ => {
                warnings.push(format!(
                    "appearance.background « {} » inconnu : « auto » utilisé.",
                    self.appearance.background
                ));
                "auto"
            }
        }
        .to_string();

        self.events.refresh_minutes = clamp_with_warning(
            self.events.refresh_minutes,
            0,
            1440,
            "events.refresh_minutes",
            &mut warnings,
        );
        self.events.timeout_seconds = clamp_with_warning(
            self.events.timeout_seconds,
            1,
            120,
            "events.timeout_seconds",
            &mut warnings,
        );

        match Rgb::parse_hex(&self.colors.today) {
            Some(c) => self.colors.today = c.to_hex(),
            None => {
                warnings.push(format!(
                    "colors.today « {} » n'est pas une couleur hexadécimale.",
                    self.colors.today
                ));
                self.colors.today = "#f5c542".into();
            }
        }

        if !self.colors.palette.is_empty() {
            if self.colors.palette.len() != 12 {
                warnings.push(format!(
                    "colors.palette doit contenir 12 couleurs ({} fournie(s)) : palette par défaut utilisée.",
                    self.colors.palette.len()
                ));
                self.colors.palette.clear();
            } else {
                let mut ok = true;
                for (i, color) in self.colors.palette.iter_mut().enumerate() {
                    match Rgb::parse_hex(color) {
                        Some(c) => *color = c.to_hex(),
                        None => {
                            warnings.push(format!(
                                "colors.palette[{i}] « {color} » n'est pas une couleur hexadécimale."
                            ));
                            ok = false;
                        }
                    }
                }
                if !ok {
                    self.colors.palette.clear();
                }
            }
        }

        let mut rebuilt = BTreeMap::new();
        for (key, mut src) in std::mem::take(&mut self.calendars) {
            let key = key.trim().to_string();
            if key.is_empty() {
                warnings.push("Un calendrier sans nom de table a été ignoré.".into());
                continue;
            }
            src.name = src.name.trim().to_string();
            src.url = src.url.trim().to_string();
            if !src.url.is_empty() {
                let lower = src.url.to_ascii_lowercase();
                let scheme_ok = lower.starts_with("http://")
                    || lower.starts_with("https://")
                    || lower.starts_with("webcal://");
                if !scheme_ok {
                    warnings.push(format!(
                        "calendars.{key}.url : schéma non supporté (attendu http://, https:// ou webcal://)."
                    ));
                } else if lower.contains("calendar.google.com/calendar/u/")
                    || (lower.contains("calendar.google.com/calendar?") && lower.contains("cid="))
                {
                    warnings.push(format!(
                        "calendars.{key}.url est une page web Google Calendar, pas un flux iCal : utilisez l'adresse secrète au format iCal (…/basic.ics)."
                    ));
                }
                if lower.starts_with("webcal://") {
                    src.url = format!("https://{}", &src.url["webcal://".len()..]);
                }
            }
            if let Some(color) = src.color.as_deref() {
                match Rgb::parse_hex(color) {
                    Some(c) => src.color = Some(c.to_hex()),
                    None => {
                        warnings.push(format!(
                            "calendars.{key}.color « {color} » invalide : couleur par défaut utilisée."
                        ));
                        src.color = None;
                    }
                }
            }
            rebuilt.insert(key, src);
        }
        self.calendars = rebuilt;

        warnings
    }

    pub fn to_settings(&self, path: PathBuf) -> Settings {
        Settings::from_config(self, path)
    }
}

fn clamp_with_warning(
    value: i64,
    lo: i64,
    hi: i64,
    field: &str,
    warnings: &mut Vec<String>,
) -> i64 {
    if value < lo || value > hi {
        let clamped = value.clamp(lo, hi);
        warnings.push(format!(
            "{field} = {value} hors limites : {clamped} utilisé (plage {lo}..={hi})."
        ));
        clamped
    } else {
        value
    }
}

#[derive(Debug)]
pub struct LoadedConfig {
    pub settings: Settings,
    pub warnings: Vec<String>,
    pub created: bool,
    /// Erreur de lecture/parsing : l'appelant peut conserver l'ancienne config.
    pub error: Option<String>,
    pub path: PathBuf,
}

/// Cherche le chemin de configuration sans ouvrir ni créer le fichier.
pub fn config_path(cli_override: Option<&Path>) -> PathBuf {
    if let Some(path) = cli_override {
        return expand_tilde(path);
    }
    if let Ok(raw) = env::var("CONTCAL_CONFIG") {
        let raw = raw.trim();
        if !raw.is_empty() {
            return expand_tilde(Path::new(raw));
        }
    }
    home_dir()
        .map(|home| home.join(CONFIG_FILENAME))
        .unwrap_or_else(|| PathBuf::from(CONFIG_FILENAME))
}

pub fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("USERPROFILE")
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
        })
        .or_else(|| {
            let drive = env::var_os("HOMEDRIVE")?;
            let path = env::var_os("HOMEPATH")?;
            let mut joined = drive;
            joined.push(path);
            Some(PathBuf::from(joined))
        })
}

fn expand_tilde(path: &Path) -> PathBuf {
    let Some(raw) = path.to_str() else {
        return path.to_path_buf();
    };
    if raw == "~" {
        return home_dir().unwrap_or_else(|| path.to_path_buf());
    }
    if let Some(rest) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\"))
        && let Some(home) = home_dir()
    {
        return home.join(rest);
    }
    path.to_path_buf()
}

/// Affiche un chemin avec `~` à la place du home.
pub fn pretty_path(path: &Path) -> String {
    if let Some(home) = home_dir()
        && let Ok(rest) = path.strip_prefix(&home)
    {
        if rest.as_os_str().is_empty() {
            return "~".into();
        }
        return format!("~/{}", rest.display());
    }
    path.display().to_string()
}

/// Charge (ou crée) le fichier, puis valide les valeurs.
///
/// En cas d'erreur, `settings` contient des valeurs par défaut utilisables et
/// `error` décrit le problème. Un fichier absent est créé avec le modèle commenté.
pub fn load(cli_override: Option<&Path>) -> LoadedConfig {
    let path = config_path(cli_override);
    let mut warnings = Vec::new();
    let mut created = false;
    let mut error = None;

    if !path.exists() {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
            && !parent.exists()
            && let Err(e) = fs::create_dir_all(parent)
        {
            error = Some(format!("Impossible de créer {} : {e}", parent.display()));
        }
        if error.is_none() {
            match fs::write(&path, DEFAULT_CONFIG) {
                Ok(()) => {
                    created = true;
                    restrict_permissions(&path);
                }
                Err(e) => {
                    error = Some(format!(
                        "Impossible d'écrire la configuration {} : {e}",
                        path.display()
                    ));
                }
            }
        }
    }

    let config = match fs::read_to_string(&path) {
        Ok(text) => match toml::from_str::<Config>(&text) {
            Ok(mut cfg) => {
                warnings.extend(cfg.validate());
                cfg
            }
            Err(e) => {
                error = Some(format!(
                    "Configuration invalide ({}), valeurs par défaut utilisées : {e}",
                    pretty_path(&path)
                ));
                Config::default()
            }
        },
        Err(e) => {
            if error.is_none() {
                error = Some(format!(
                    "Lecture impossible de {} : {e}",
                    pretty_path(&path)
                ));
            }
            Config::default()
        }
    };

    let settings = config.to_settings(path.clone());
    LoadedConfig {
        settings,
        warnings,
        created,
        error,
        path,
    }
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(text: &str) -> Config {
        let mut cfg: Config = toml::from_str(text).expect("toml valide");
        let _ = cfg.validate();
        cfg
    }

    #[test]
    fn default_does_not_reparse_the_template() {
        // Garde-fou : Config::default() ne doit pas re-parser DEFAULT_CONFIG
        // (serde pourrait rappeler Default pendant la désérialisation).
        let cfg = Config::default();
        assert_eq!(cfg.calendars.len(), 2);
        let settings = cfg.to_settings(PathBuf::from("/tmp/cfg"));
        assert_eq!(settings.sources.len(), 2);
        assert!(settings.sources.iter().all(|source| !source.is_active()));
    }

    #[test]
    fn default_template_is_valid() {
        let cfg = parsed(DEFAULT_CONFIG);
        let settings = cfg.to_settings(PathBuf::from("/tmp/cfg"));
        assert_eq!(settings.week_start, WeekStart::Monday);
        assert_eq!(settings.row_height, 1);
        assert_eq!(settings.lang, Lang::Fr);
        assert_eq!(settings.sources.len(), 2);
        assert_eq!(settings.palette, DEFAULT_PALETTE);
        assert_eq!(settings.start, StartPosition::Today);
        assert!(settings.refresh.is_some());
        // Aucune URL renseignée : aucune source active.
        assert!(settings.sources.iter().all(|s| !s.is_active()));
    }

    #[test]
    fn clamps_and_falls_back() {
        let text = r#"
[calendar]
week_start = "vendredi"
start = "32/13/2026"
language = "de"

[appearance]
row_height = 9
scroll_weeks = -3
month_tint = 99
color_mode = "sepia"

[colors]
today = "pas-une-couleur"
"#;
        let mut cfg: Config = toml::from_str(text).unwrap();
        let warnings = cfg.validate();
        assert!(!warnings.is_empty());
        assert_eq!(cfg.calendar.week_start, "monday");
        assert_eq!(cfg.calendar.start, "today");
        assert_eq!(cfg.calendar.language, "fr");
        assert_eq!(cfg.appearance.row_height, 8);
        assert_eq!(cfg.appearance.scroll_weeks, 1);
        assert_eq!(cfg.appearance.month_tint, 45);
        assert_eq!(cfg.appearance.color_mode, "auto");
        assert_eq!(cfg.colors.today, "#f5c542");
    }

    #[test]
    fn accepts_french_week_start_and_date() {
        let mut cfg: Config = toml::from_str(
            r#"
[calendar]
week_start = "dimanche"
start = "06/07/2026"
"#,
        )
        .unwrap();
        assert!(cfg.validate().is_empty());
        assert_eq!(cfg.calendar.week_start, "sunday");
        assert_eq!(cfg.calendar.start, "2026-07-06");
        let settings = cfg.to_settings(PathBuf::from("x"));
        assert_eq!(settings.week_start, WeekStart::Sunday);
        assert_eq!(
            settings.start,
            StartPosition::On(NaiveDate::from_ymd_opt(2026, 7, 6).unwrap())
        );
    }

    #[test]
    fn webcal_is_rewritten_and_colors_validated() {
        let mut cfg: Config = toml::from_str(
            r##"
[calendars.outlook]
url = "webcal://example.com/cal.ics"
color = "#abc"

[calendars.google]
url = "ftp://example.com/cal.ics"
color = "wat"
"##,
        )
        .unwrap();
        let warnings = cfg.validate();
        assert_eq!(cfg.calendars["outlook"].url, "https://example.com/cal.ics");
        assert_eq!(cfg.calendars["outlook"].color.as_deref(), Some("#aabbcc"));
        assert_eq!(cfg.calendars["google"].color, None);
        assert!(warnings.iter().any(|w| w.contains("calendars.google.url")));
        assert!(warnings.iter().any(|w| w.contains("google.color")));
    }

    #[test]
    fn google_web_link_is_flagged() {
        let mut cfg: Config = toml::from_str(
            r#"
[calendars.google]
url = "https://calendar.google.com/calendar/u/0?cid=YWJj"
"#,
        )
        .unwrap();
        let warnings = cfg.validate();
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("page web Google Calendar"))
        );
    }

    #[test]
    fn palette_requires_twelve_colors() {
        let mut cfg: Config = toml::from_str(
            r##"
[colors]
palette = ["#fff", "#000"]
"##,
        )
        .unwrap();
        let warnings = cfg.validate();
        assert!(warnings.iter().any(|w| w.contains("12 couleurs")));
        let settings = cfg.to_settings(PathBuf::from("x"));
        assert_eq!(settings.palette, DEFAULT_PALETTE);
    }

    #[test]
    fn source_activation_requires_url() {
        let src = Source {
            key: "outlook".into(),
            name: "Outlook".into(),
            url: "  ".into(),
            enabled: true,
            color: Rgb::BLACK,
        };
        assert!(!src.is_active());
    }
}
