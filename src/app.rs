//! État de l'application : vue, calendriers, cache d'événements, entrées.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use chrono::{Local, NaiveDate, NaiveTime};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind};

use crate::calendar;
use crate::color::Rgb;
use crate::config::{self, LoadedConfig, Settings, Source};
use crate::fetch::{self, FetchResult};
use crate::ics::{self, Event};
use crate::lang::Lang;
use crate::layout::Layout;
use crate::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceStatus {
    /// Source désactivée dans la configuration.
    Disabled,
    /// Source activée mais sans URL.
    MissingUrl,
    /// En attente du premier téléchargement.
    Idle,
    Loading,
    Loaded,
    Error,
}

#[derive(Debug)]
pub struct SourceState {
    pub source: Source,
    pub events: Vec<Event>,
    pub status: SourceStatus,
    pub last_success: Option<Instant>,
    pub loading_since: Option<Instant>,
    pub error: Option<String>,
    pub warnings: Vec<String>,
}

impl SourceState {
    pub fn new(source: Source) -> Self {
        let status = if !source.enabled {
            SourceStatus::Disabled
        } else if source.url.trim().is_empty() {
            SourceStatus::MissingUrl
        } else {
            SourceStatus::Idle
        };
        Self {
            source,
            events: Vec::new(),
            status,
            last_success: None,
            loading_since: None,
            error: None,
            warnings: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DisplayEvent {
    pub title: String,
    pub start_time: Option<NaiveTime>,
    pub all_day: bool,
    pub color: Rgb,
    pub continues: bool,
}

#[derive(Debug)]
pub struct EventCache {
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub data_generation: u64,
    pub days: HashMap<NaiveDate, Vec<DisplayEvent>>,
    /// Nombre d'événements-jours visibles (les événements longs comptent par jour).
    pub total: usize,
}

impl EventCache {
    pub fn events_on(&self, date: NaiveDate) -> &[DisplayEvent] {
        self.days.get(&date).map(Vec::as_slice).unwrap_or(&[])
    }
}

const MAX_EVENTS_PER_DAY: usize = 64;

/// Construit le cache d'affichage pour la plage visible.
pub fn build_cache(
    sources: &[SourceState],
    from: NaiveDate,
    to: NaiveDate,
    generation: u64,
) -> EventCache {
    let mut days: HashMap<NaiveDate, Vec<DisplayEvent>> = HashMap::new();
    let mut total = 0usize;
    if from <= to {
        for source in sources {
            for event in &source.events {
                for start in ics::occurrence_days(event, from, to) {
                    for offset in 0..event.span_days.max(1) {
                        let date = start + chrono::Duration::days(offset);
                        if date < from || date > to {
                            continue;
                        }
                        let list = days.entry(date).or_default();
                        if list.len() >= MAX_EVENTS_PER_DAY {
                            continue;
                        }
                        list.push(DisplayEvent {
                            title: event.summary.clone(),
                            start_time: event.start_time,
                            all_day: event.all_day,
                            color: source.source.color,
                            continues: offset > 0,
                        });
                        total += 1;
                    }
                }
            }
        }
    }
    for list in days.values_mut() {
        list.sort_by(|a, b| {
            b.all_day
                .cmp(&a.all_day)
                .then_with(|| a.start_time.cmp(&b.start_time))
                .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
        });
    }
    EventCache {
        from,
        to,
        data_generation: generation,
        days,
        total,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub text: String,
    pub kind: ToastKind,
    pub until: Instant,
}

#[derive(Debug)]
pub struct App {
    pub settings: Settings,
    pub theme: Theme,
    pub size: (u16, u16),
    pub layout: Option<Layout>,
    pub anchor: NaiveDate,
    pub today: NaiveDate,
    pub info_open: bool,
    pub quit: bool,
    pub sources: Vec<SourceState>,
    pub cache: Option<EventCache>,
    pub data_generation: u64,
    pub fetch_generation: u64,
    pub tx: Sender<FetchResult>,
    pub rx: Receiver<FetchResult>,
    pub last_refresh: Option<Instant>,
    pub config_warnings: Vec<String>,
    pub config_error: Option<String>,
    pub config_override: Option<PathBuf>,
    pub toast: Option<Toast>,
    pub dirty: bool,
}

impl App {
    pub fn new(
        loaded: LoadedConfig,
        config_override: Option<PathBuf>,
        width: u16,
        height: u16,
    ) -> Self {
        let LoadedConfig {
            settings,
            warnings,
            created,
            error,
            path,
        } = loaded;
        let theme = Theme::new(&settings);
        let size = (width.max(1), height.max(1));
        let layout = Layout::compute(size.0, size.1, settings.row_height, settings.week_numbers);
        let weeks = layout.as_ref().map(|l| l.weeks).unwrap_or(1);
        let today = Local::now().date_naive();
        let anchor = match settings.start {
            config::StartPosition::Today => {
                calendar::today_anchor(today, weeks, settings.week_start)
            }
            config::StartPosition::On(_) => {
                calendar::snap_week_start(settings.start.resolve(today), settings.week_start)
            }
        };
        let sources = settings
            .sources
            .iter()
            .cloned()
            .map(SourceState::new)
            .collect();
        let config_error = error;
        let (tx, rx) = mpsc::channel();
        let mut app = Self {
            theme,
            size,
            layout,
            anchor,
            today,
            info_open: false,
            quit: false,
            sources,
            cache: None,
            data_generation: 0,
            fetch_generation: 0,
            tx,
            rx,
            last_refresh: None,
            config_warnings: warnings,
            config_error,
            config_override,
            toast: None,
            dirty: true,
            settings,
        };

        if created {
            let msg = match app.settings.lang {
                Lang::Fr => format!(
                    "Configuration créée : {} — ajoutez vos liens ICS.",
                    config::pretty_path(&path)
                ),
                Lang::En => format!(
                    "Config created: {} — add your ICS links.",
                    config::pretty_path(&path)
                ),
            };
            app.set_toast(ToastKind::Info, msg, 12);
        }
        app
    }

    // ── temps / rafraîchissement ────────────────────────────────────────────

    pub fn tick(&mut self) {
        let today = Local::now().date_naive();
        if today != self.today {
            self.today = today;
            self.dirty = true;
        }
        self.poll_fetch();
        self.check_stalled_fetches();
        self.maybe_auto_refresh();
        self.ensure_cache();
        if self
            .toast
            .as_ref()
            .is_some_and(|t| Instant::now() >= t.until)
        {
            self.toast = None;
            self.dirty = true;
        }
    }

    fn poll_fetch(&mut self) {
        while let Ok(result) = self.rx.try_recv() {
            if result.generation != self.fetch_generation {
                continue;
            }
            let Some(state) = self
                .sources
                .iter_mut()
                .find(|s| s.source.key == result.source_key)
            else {
                continue;
            };
            match result.outcome {
                Ok(fetched) => {
                    state.events = fetched.events;
                    state.warnings = fetched.warnings;
                    state.status = SourceStatus::Loaded;
                    state.error = None;
                    state.loading_since = None;
                    state.last_success = Some(fetched.fetched_at);
                }
                Err(message) => {
                    state.status = SourceStatus::Error;
                    state.error = Some(message);
                    state.loading_since = None;
                }
            }
            self.data_generation = self.data_generation.wrapping_add(1);
            self.dirty = true;
        }
    }

    fn check_stalled_fetches(&mut self) {
        let limit = self.settings.timeout + Duration::from_secs(15);
        for source in &mut self.sources {
            if source.status == SourceStatus::Loading
                && source.loading_since.is_some_and(|t| t.elapsed() > limit)
            {
                source.status = SourceStatus::Error;
                source.error = Some("délai de téléchargement dépassé".into());
                source.loading_since = None;
                self.dirty = true;
            }
        }
    }

    fn maybe_auto_refresh(&mut self) {
        let Some(interval) = self.settings.refresh else {
            return;
        };
        if self
            .sources
            .iter()
            .any(|s| s.status == SourceStatus::Loading)
        {
            return;
        }
        if !self.sources.iter().any(|s| s.source.is_active()) {
            return;
        }
        let due = self
            .last_refresh
            .map(|t| t.elapsed() >= interval)
            .unwrap_or(true);
        if due {
            self.refresh();
        }
    }

    pub fn refresh(&mut self) {
        if !self.sources.iter().any(|s| s.source.is_active()) {
            return;
        }
        self.fetch_generation = self.fetch_generation.wrapping_add(1);
        let generation = self.fetch_generation;
        let timeout = self.settings.timeout;
        let tx = self.tx.clone();
        for state in &mut self.sources {
            if !state.source.is_active() {
                continue;
            }
            state.status = SourceStatus::Loading;
            state.loading_since = Some(Instant::now());
            state.error = None;
            fetch::spawn(state.source.clone(), generation, timeout, tx.clone());
        }
        self.last_refresh = Some(Instant::now());
        self.dirty = true;
    }

    pub fn refresh_with_feedback(&mut self) {
        if self.sources.iter().any(|s| s.source.is_active()) {
            self.refresh();
        } else {
            let msg = self.settings.lang.t("no_remote").to_string();
            self.set_toast(ToastKind::Warning, msg, 6);
        }
    }

    pub fn reload_config(&mut self) {
        let loaded = config::load(self.config_override.as_deref());
        self.apply_config(loaded);
    }

    pub fn apply_config(&mut self, loaded: LoadedConfig) {
        if let Some(error) = loaded.error {
            self.set_toast(
                ToastKind::Error,
                format!("{} : {error}", self.settings.lang.t("reload_error")),
                12,
            );
            self.dirty = true;
            return;
        }

        let old_sources = std::mem::take(&mut self.sources);
        let mut previous: HashMap<String, SourceState> = old_sources
            .into_iter()
            .map(|state| (state.source.key.clone(), state))
            .collect();
        let old_week_start = self.settings.week_start;
        let was_created = loaded.created;
        let config_path = loaded.path.clone();

        self.settings = loaded.settings;
        if old_week_start != self.settings.week_start {
            self.anchor = calendar::snap_week_start(self.anchor, self.settings.week_start);
        }
        self.theme = Theme::new(&self.settings);
        self.config_warnings = loaded.warnings;
        self.config_error = None;
        self.fetch_generation = self.fetch_generation.wrapping_add(1);
        self.last_refresh = None;

        self.sources = self
            .settings
            .sources
            .iter()
            .map(|src| {
                let mut state = SourceState::new(src.clone());
                if let Some(previous) = previous.remove(&src.key)
                    && previous.source.url == src.url
                    && src.enabled
                {
                    state.events = previous.events;
                    state.last_success = previous.last_success;
                    state.warnings = previous.warnings;
                    if !state.events.is_empty() {
                        state.status = SourceStatus::Loaded;
                    }
                }
                state
            })
            .collect();

        self.recompute_layout();
        self.data_generation = self.data_generation.wrapping_add(1);
        self.cache = None;
        let warnings = self.config_warnings.len();
        let message = if was_created {
            match self.settings.lang {
                Lang::Fr => format!(
                    "Configuration créée : {} — ajoutez vos liens ICS.",
                    config::pretty_path(&config_path)
                ),
                Lang::En => format!(
                    "Config created: {} — add your ICS links.",
                    config::pretty_path(&config_path)
                ),
            }
        } else {
            match (self.settings.lang, warnings) {
                (Lang::Fr, 0) => "Configuration rechargée.".to_string(),
                (Lang::Fr, n) => format!("Configuration rechargée ({n} avertissement(s))."),
                (Lang::En, 0) => "Configuration reloaded.".to_string(),
                (Lang::En, n) => format!("Configuration reloaded ({n} warning(s))."),
            }
        };
        self.set_toast(
            if warnings == 0 || was_created {
                ToastKind::Success
            } else {
                ToastKind::Warning
            },
            message,
            6,
        );
        self.refresh();
        self.dirty = true;
    }

    fn recompute_layout(&mut self) {
        self.layout = Layout::compute(
            self.size.0,
            self.size.1,
            self.settings.row_height,
            self.settings.week_numbers,
        );
        self.cache = None;
        self.dirty = true;
    }

    pub fn resize(&mut self, width: u16, height: u16) {
        if (width, height) == self.size {
            return;
        }
        self.size = (width.max(1), height.max(1));
        self.recompute_layout();
    }

    // ── cache ───────────────────────────────────────────────────────────────

    pub fn ensure_cache(&mut self) {
        let Some(layout) = self.layout.as_ref() else {
            self.cache = None;
            return;
        };
        if !self.settings.show_events {
            self.cache = None;
            return;
        }
        let from = self.anchor;
        let to = calendar::last_visible(self.anchor, layout.weeks);
        let needs_rebuild = self.cache.as_ref().is_none_or(|cache| {
            cache.from != from || cache.to != to || cache.data_generation != self.data_generation
        });
        if needs_rebuild {
            self.cache = Some(build_cache(&self.sources, from, to, self.data_generation));
        }
    }

    pub fn events_for(&self, date: NaiveDate) -> &[DisplayEvent] {
        self.cache
            .as_ref()
            .map(|cache| cache.events_on(date))
            .unwrap_or(&[])
    }

    pub fn visible_event_count(&self) -> usize {
        self.cache.as_ref().map(|cache| cache.total).unwrap_or(0)
    }

    pub fn total_event_count(&self) -> usize {
        self.sources.iter().map(|s| s.events.len()).sum()
    }

    pub fn active_source_count(&self) -> usize {
        self.sources.iter().filter(|s| s.source.is_active()).count()
    }

    pub fn visible_weeks(&self) -> u16 {
        self.layout.as_ref().map(|l| l.weeks).unwrap_or(1)
    }

    pub fn first_visible(&self) -> NaiveDate {
        self.anchor
    }

    pub fn last_visible(&self) -> NaiveDate {
        calendar::last_visible(self.anchor, self.visible_weeks())
    }

    // ── navigation ─────────────────────────────────────────────────────────

    pub fn scroll_weeks(&mut self, delta: i64) {
        if delta == 0 {
            return;
        }
        let anchor = calendar::add_weeks(self.anchor, delta);
        if anchor != self.anchor {
            self.anchor = anchor;
            self.dirty = true;
        }
    }

    pub fn scroll_step(&mut self, direction: i64) {
        let step = self.settings.scroll_weeks.max(1);
        self.scroll_weeks(direction.saturating_mul(step));
    }

    pub fn page(&mut self, direction: i64) {
        let step = i64::from((self.visible_weeks() / 2).max(1));
        self.scroll_weeks(direction.saturating_mul(step));
    }

    pub fn shift_month(&mut self, delta: i32) {
        let anchor = calendar::add_months(self.anchor, delta, self.settings.week_start);
        if anchor != self.anchor {
            self.anchor = anchor;
            self.dirty = true;
        }
    }

    pub fn jump_today(&mut self) {
        let anchor =
            calendar::today_anchor(self.today, self.visible_weeks(), self.settings.week_start);
        if anchor != self.anchor {
            self.anchor = anchor;
            self.dirty = true;
        }
    }

    // ── entrées ────────────────────────────────────────────────────────────

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && matches!(key.code, KeyCode::Char('c')) {
            self.quit = true;
            return;
        }
        if self.info_open {
            match key.code {
                KeyCode::Esc | KeyCode::Char('i') | KeyCode::Char('?') | KeyCode::F(1) => {
                    self.info_open = false;
                    self.dirty = true;
                }
                KeyCode::Char('q') => self.quit = true,
                KeyCode::Char('r') => self.refresh_with_feedback(),
                KeyCode::Char('c') => self.reload_config(),
                _ => {}
            }
            return;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            KeyCode::Up | KeyCode::Char('k') => self.scroll_weeks(-1),
            KeyCode::Down | KeyCode::Char('j') => self.scroll_weeks(1),
            KeyCode::PageUp => self.page(-1),
            KeyCode::PageDown => self.page(1),
            KeyCode::Left | KeyCode::Char('h') => self.shift_month(-1),
            KeyCode::Right | KeyCode::Char('l') => self.shift_month(1),
            KeyCode::Home | KeyCode::Char('t') => self.jump_today(),
            KeyCode::Char('g') => {
                self.anchor = calendar::min_anchor();
                self.dirty = true;
            }
            KeyCode::Char('G') => {
                self.anchor =
                    calendar::snap_week_start(calendar::max_anchor(), self.settings.week_start);
                self.dirty = true;
            }
            KeyCode::Char('r') => self.refresh_with_feedback(),
            KeyCode::Char('c') => self.reload_config(),
            KeyCode::Char('i') | KeyCode::Char('?') | KeyCode::F(1) => {
                self.info_open = true;
                self.dirty = true;
            }
            _ => {}
        }
    }

    pub fn handle_mouse(&mut self, mouse: MouseEvent) {
        if self.info_open {
            return;
        }
        match mouse.kind {
            MouseEventKind::ScrollUp => self.scroll_step(-1),
            MouseEventKind::ScrollDown => self.scroll_step(1),
            _ => {}
        }
    }

    pub fn current_toast(&self) -> Option<&Toast> {
        self.toast
            .as_ref()
            .filter(|toast| Instant::now() < toast.until)
    }

    pub fn set_toast(&mut self, kind: ToastKind, text: String, seconds: u64) {
        self.toast = Some(Toast {
            text,
            kind,
            until: Instant::now() + Duration::from_secs(seconds),
        });
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveTime;

    fn source_state(key: &str) -> SourceState {
        let mut source = SourceState::new(Source {
            key: key.into(),
            name: key.into(),
            url: format!("https://example.com/{key}.ics"),
            enabled: true,
            color: Rgb::new(10, 20, 30),
        });
        source.status = SourceStatus::Loaded;
        source
    }

    fn event(start: NaiveDate, span: i64, title: &str) -> Event {
        Event {
            uid: format!("uid-{title}"),
            summary: title.into(),
            location: None,
            start,
            start_time: Some(NaiveTime::from_hms_opt(9, 0, 0).unwrap()),
            span_days: span,
            all_day: false,
            rrule: None,
            exdates: Default::default(),
            rdates: Vec::new(),
            recurrence_id: None,
        }
    }

    #[test]
    fn cache_places_multiday_events_on_each_day() {
        let d = NaiveDate::from_ymd_opt(2026, 7, 6).unwrap();
        let mut source = source_state("outlook");
        source.events.push(event(d, 2, "Congrès"));
        let cache = build_cache(
            &[source],
            d,
            NaiveDate::from_ymd_opt(2026, 7, 12).unwrap(),
            0,
        );
        assert_eq!(cache.total, 2);
        assert_eq!(cache.events_on(d)[0].title, "Congrès");
        assert!(!cache.events_on(d)[0].continues);
        assert!(cache.events_on(d + chrono::Duration::days(1))[0].continues);
        assert!(cache.events_on(d + chrono::Duration::days(2)).is_empty());
    }

    #[test]
    fn cache_filters_outside_range() {
        let d = NaiveDate::from_ymd_opt(2026, 7, 6).unwrap();
        let mut source = source_state("google");
        source.events.push(event(d, 1, "Passé"));
        source.events.push(event(
            NaiveDate::from_ymd_opt(2026, 8, 1).unwrap(),
            1,
            "Futur",
        ));
        let cache = build_cache(&[source], d, d + chrono::Duration::days(6), 0);
        assert_eq!(cache.total, 1);
        assert_eq!(cache.days.len(), 1);
    }

    #[test]
    fn cache_sorts_all_day_first() {
        let d = NaiveDate::from_ymd_opt(2026, 7, 6).unwrap();
        let mut source = source_state("google");
        source.events.push(event(d, 1, "Réunion"));
        let mut holiday = event(d, 1, "Férié");
        holiday.all_day = true;
        holiday.start_time = None;
        source.events.push(holiday);
        let cache = build_cache(&[source], d, d, 0);
        let events = cache.events_on(d);
        assert_eq!(events[0].title, "Férié");
        assert_eq!(events[1].title, "Réunion");
    }
}
