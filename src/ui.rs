//! Rendu ANSI de l'interface : calendrier continu, pied de page, écran d'aide.

use std::io::{self, Write};

use chrono::{Datelike, NaiveDate};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{App, DisplayEvent, SourceState, SourceStatus, ToastKind};
use crate::calendar;
use crate::color::Rgb;
use crate::config::pretty_path;
use crate::fetch;
use crate::lang::Lang;
use crate::layout::{Layout, MIN_HEIGHT, MIN_WIDTH};
use crate::theme::{ColorMode, Theme};

const VERSION: &str = env!("CARGO_PKG_VERSION");

// ─────────────────────────────────────────────────────────────────────────────
//  Styles et texte
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Style {
    fg: Option<Rgb>,
    bg: Option<Rgb>,
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
    reverse: bool,
}

impl Style {
    fn fg(color: Rgb) -> Self {
        Self {
            fg: Some(color),
            ..Self::default()
        }
    }

    fn with_bg(mut self, bg: Option<Rgb>) -> Self {
        self.bg = bg;
        self
    }

    fn with_bold(mut self) -> Self {
        self.bold = true;
        self
    }

    fn with_underline(mut self) -> Self {
        self.underline = true;
        self
    }

    fn with_reverse(mut self) -> Self {
        self.reverse = true;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Span {
    text: String,
    style: Style,
}

impl Span {
    fn new(text: impl Into<String>, style: Style) -> Self {
        Self {
            text: sanitize(&text.into()),
            style,
        }
    }
}

fn spaces(n: usize, style: Style) -> Span {
    Span {
        text: " ".repeat(n),
        style,
    }
}

fn sanitize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\n' | '\r' | '\t' => out.push(' '),
            '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{200e}' | '\u{200f}' => {
                out.push(' ')
            }
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

fn clean_title(text: &str) -> String {
    let cleaned = sanitize(text);
    let joined = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    if joined.is_empty() {
        "…".to_string()
    } else {
        joined
    }
}

fn width_of(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

fn spans_width(spans: &[Span]) -> usize {
    spans.iter().map(|s| width_of(&s.text)).sum()
}

/// Tronque à `max` colonnes, avec une ellipse si nécessaire.
fn limit_width(text: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if width_of(text) <= max {
        return text.to_string();
    }
    let budget = max.saturating_sub(1);
    let mut out = String::new();
    let mut used = 0usize;
    for ch in text.chars() {
        let w = ch.width().unwrap_or(0);
        if used + w > budget {
            break;
        }
        out.push(ch);
        used += w;
    }
    out.push('…');
    out
}

fn pad_right(text: &str, width: usize) -> String {
    let mut out = limit_width(text, width);
    let w = width_of(&out);
    if w < width {
        out.push_str(&" ".repeat(width - w));
    }
    out
}

fn push_fit(
    spans: &mut Vec<Span>,
    text: &str,
    style: Style,
    used: &mut usize,
    width: usize,
) -> bool {
    let remaining = width.saturating_sub(*used);
    if remaining == 0 {
        return false;
    }
    let fitted = limit_width(text, remaining);
    let w = width_of(&fitted);
    if w == 0 {
        return false;
    }
    spans.push(Span::new(fitted, style));
    *used += w;
    true
}

/// Ajoute le contenu d'une cellule puis complète avec le fond de la cellule.
fn append_block(dst: &mut Vec<Span>, content: Vec<Span>, width: u16, bg: Option<Rgb>) {
    let target = usize::from(width);
    let mut used = 0usize;
    for mut span in content {
        if used >= target {
            break;
        }
        let text = limit_width(&span.text, target - used);
        let w = width_of(&text);
        if w == 0 {
            continue;
        }
        if span.style.bg.is_none() {
            span.style.bg = bg;
        }
        dst.push(Span::new(text, span.style));
        used += w;
    }
    if used < target {
        dst.push(spaces(target - used, Style::default().with_bg(bg)));
    }
}

fn ansi_prefix(style: Style, theme: &Theme) -> String {
    let mut out = String::from("\x1b[0m");
    if theme.mode != ColorMode::Mono {
        if let Some(fg) = style.fg {
            out.push_str(&fg.fg_code(theme.truecolor()));
        }
        if let Some(bg) = style.bg {
            out.push_str(&bg.bg_code(theme.truecolor()));
        }
    }
    if style.bold {
        out.push_str("\x1b[1m");
    }
    if style.dim {
        out.push_str("\x1b[2m");
    }
    if style.italic {
        out.push_str("\x1b[3m");
    }
    if style.underline {
        out.push_str("\x1b[4m");
    }
    if style.reverse {
        out.push_str("\x1b[7m");
    }
    out
}

fn render_spans(spans: &[Span], width: u16, default: Style, theme: &Theme) -> String {
    let target = usize::from(width);
    let mut out = String::new();
    let mut used = 0usize;
    for span in spans {
        if used >= target {
            break;
        }
        let text = limit_width(&span.text, target - used);
        let w = width_of(&text);
        if w == 0 {
            continue;
        }
        out.push_str(&ansi_prefix(span.style, theme));
        out.push_str(&text);
        used += w;
    }
    if used < target {
        out.push_str(&ansi_prefix(default, theme));
        out.push_str(&" ".repeat(target - used));
    }
    out.push_str("\x1b[0m");
    out
}

fn blank_line(width: u16, theme: &Theme) -> String {
    render_spans(&[], width, Style::default(), theme)
}

// ─────────────────────────────────────────────────────────────────────────────
//  Point d'entrée du rendu
// ─────────────────────────────────────────────────────────────────────────────

pub fn render(app: &App) -> Vec<String> {
    if app.layout.is_none() {
        return too_small_lines(app);
    }
    if app.info_open {
        info_lines(app)
    } else {
        calendar_lines(app)
    }
}

/// Peint un cadre complet dans le terminal (sortie synchronisée si possible).
pub fn draw(lines: &[String]) -> io::Result<()> {
    if lines.is_empty() {
        return Ok(());
    }
    let height = crossterm::terminal::size()
        .map(|(_, h)| usize::from(h))
        .unwrap_or(lines.len())
        .max(1);
    let visible = lines.len().min(height);

    let mut out = String::with_capacity(lines.iter().map(|l| l.len() + 9).sum());
    out.push_str("\x1b[?2026h\x1b[H");
    for (index, line) in lines.iter().take(visible).enumerate() {
        out.push_str(line);
        out.push_str("\x1b[0m\x1b[K");
        if index + 1 < visible {
            out.push_str("\r\n");
        }
    }
    out.push_str("\x1b[?2026l");

    let mut stdout = io::stdout().lock();
    stdout.write_all(out.as_bytes())?;
    stdout.flush()
}

// ─────────────────────────────────────────────────────────────────────────────
//  Calendrier
// ─────────────────────────────────────────────────────────────────────────────

struct Cell {
    bg: Option<Rgb>,
    lines: Vec<Vec<Span>>,
}

fn calendar_lines(app: &App) -> Vec<String> {
    let Some(layout) = app.layout.as_ref() else {
        return too_small_lines(app);
    };
    let theme = &app.theme;
    let mut lines = Vec::with_capacity(usize::from(layout.height));

    lines.push(top_bar(app, layout));
    lines.push(week_header(app, layout));

    for week in 0..layout.weeks {
        let anchor = app.anchor + chrono::Duration::days(i64::from(week) * 7);
        let days = calendar::row_days(anchor);
        // La dernière rangée peut être partielle (4 lignes sur 8, par exemple).
        let row_height = layout.week_height(week);
        let cells: [Cell; 7] =
            std::array::from_fn(|i| build_cell(app, days[i], layout.cell_widths[i], row_height));
        let first_of_month = calendar::first_of_month_in_row(anchor);
        for line_idx in 0..row_height {
            lines.push(week_line(
                app,
                layout,
                anchor,
                first_of_month,
                &cells,
                usize::from(line_idx),
            ));
        }
    }

    lines.extend(footer_lines(app, layout));
    debug_assert!(lines.len() <= usize::from(layout.height));
    while lines.len() < usize::from(layout.height) {
        lines.push(blank_line(layout.width, theme));
    }
    lines
}

fn top_bar(app: &App, layout: &Layout) -> String {
    let theme = &app.theme;
    let lang = app.settings.lang;
    let accent = theme.accent(app.anchor.month());
    let bar_bg = if theme.mode == ColorMode::Mono {
        None
    } else {
        Some(theme.base.blend(accent, if theme.dark { 14 } else { 9 }))
    };

    let brand = Style::fg(accent.readable_text())
        .with_bg(Some(accent))
        .with_bold();
    let mut left = vec![
        Span::new(" contcal ", brand),
        spaces(2, Style::default()),
        Span::new(
            calendar::range_label(lang, app.first_visible(), app.last_visible()),
            Style::fg(theme.fg).with_bold(),
        ),
    ];

    let mut right_full = vec![
        Span::new("◉ ", Style::fg(theme.today_fg()).with_bold()),
        Span::new(lang.date_short(app.today), Style::fg(theme.muted)),
    ];
    if app.settings.show_events {
        let count = app.visible_event_count();
        if count > 0 {
            right_full.push(Span::new(
                format!("  ·  {}", event_phrase(lang, count)),
                Style::fg(theme.muted),
            ));
        }
    }
    let right_date = vec![
        Span::new("◉ ", Style::fg(theme.today_fg()).with_bold()),
        Span::new(lang.date_short(app.today), Style::fg(theme.muted)),
    ];

    let width = usize::from(layout.width);
    let left_width = spans_width(&left);
    let right = [right_full, right_date, Vec::new()]
        .into_iter()
        .find(|candidate| left_width + 2 + spans_width(candidate) <= width)
        .unwrap_or_default();

    // Une marque tout en couleur puis une barre très légèrement teintée.
    if theme.mode == ColorMode::Mono {
        // L'accent n'est pas visible : on garde le contraste par la graisse.
        left[0].style = Style::default().with_bold();
    }
    compose_bar(left, right, layout.width, bar_bg, theme)
}

fn compose_bar(
    left: Vec<Span>,
    right: Vec<Span>,
    width: u16,
    bar_bg: Option<Rgb>,
    theme: &Theme,
) -> String {
    let mut left = left;
    let mut right = right;
    for span in left.iter_mut().chain(right.iter_mut()) {
        if span.style.fg.is_none() {
            span.style.fg = Some(theme.fg);
        }
        if span.style.bg.is_none() {
            span.style.bg = bar_bg;
        }
    }
    let target = usize::from(width);
    let left_width = spans_width(&left);
    let right_width = spans_width(&right);
    let mut spans = left;
    if right_width > 0 && left_width + 2 + right_width <= target {
        let filler = target - left_width - right_width;
        spans.push(spaces(filler.max(2), Style::default().with_bg(bar_bg)));
        spans.extend(right);
    }
    render_spans(
        &spans,
        width,
        Style {
            fg: Some(theme.fg),
            bg: bar_bg,
            ..Style::default()
        },
        theme,
    )
}

fn week_header(app: &App, layout: &Layout) -> String {
    let theme = &app.theme;
    let lang = app.settings.lang;
    let mut spans = Vec::new();

    if layout.gutter_width > 0 {
        let label = pad_right(lang.week_label(), usize::from(layout.gutter_width));
        append_block(
            &mut spans,
            vec![Span::new(label, Style::fg(theme.faint))],
            layout.gutter_width,
            None,
        );
        if layout.gutter_gap > 0 {
            spans.push(spaces(usize::from(layout.gutter_gap), Style::default()));
        }
    }

    let start = calendar::start_index(app.settings.week_start);
    for (i, cell_width) in layout.cell_widths.iter().enumerate() {
        let weekday = (start + i) % 7;
        let full = lang.weekdays()[weekday].to_uppercase();
        let short = lang.weekdays_short()[weekday].to_uppercase();
        let cw = usize::from(*cell_width);
        let label = if width_of(&full) < cw && cw >= 8 {
            full
        } else if width_of(&short) < cw {
            short
        } else {
            lang.weekdays_short()[weekday]
                .chars()
                .next()
                .unwrap_or('?')
                .to_uppercase()
                .to_string()
        };
        let style = if weekday >= 5 {
            Style::fg(theme.faint)
        } else {
            Style::fg(theme.muted)
        };
        let mut content = vec![spaces(1, Style::default()), Span::new(label, style)];
        append_block(&mut spans, std::mem::take(&mut content), *cell_width, None);
        if i < 6 {
            spans.push(separator_span(theme));
        }
    }

    render_spans(&spans, layout.width, Style::default(), theme)
}

fn separator_span(theme: &Theme) -> Span {
    if theme.mode == ColorMode::Mono || theme.tint == 0 {
        Span::new("│", Style::fg(theme.faint))
    } else {
        spaces(1, Style::default())
    }
}

fn gutter_spans(
    app: &App,
    layout: &Layout,
    anchor: NaiveDate,
    first_of_month: Option<NaiveDate>,
    line_idx: usize,
) -> Vec<Span> {
    if layout.gutter_width == 0 {
        return Vec::new();
    }
    let theme = &app.theme;
    let lang = app.settings.lang;
    let width = usize::from(layout.gutter_width);
    let mut spans = Vec::new();

    if line_idx == 0 {
        let label = format!("{}{:02}", lang.week_prefix(), calendar::week_number(anchor));
        let week_has_today = app.today >= anchor && app.today < anchor + chrono::Duration::days(7);
        let style = if week_has_today {
            Style::fg(theme.today_fg()).with_bold()
        } else if let Some(day) = first_of_month {
            Style::fg(theme.accent_fg(day.month())).with_bold()
        } else {
            Style::fg(theme.faint)
        };
        spans.push(Span::new(format!("{label:>width$}"), style));
    } else if line_idx == 1
        && layout.row_height > 1
        && let Some(day) = first_of_month
    {
        let name = lang.months_short()[day.month() as usize - 1];
        spans.push(Span::new(
            format!("{name:>width$}"),
            Style::fg(theme.accent_fg(day.month())).with_bold(),
        ));
    }
    spans
}

fn week_line(
    app: &App,
    layout: &Layout,
    anchor: NaiveDate,
    first_of_month: Option<NaiveDate>,
    cells: &[Cell; 7],
    line_idx: usize,
) -> String {
    let theme = &app.theme;
    let mut spans = Vec::new();

    let gutter = gutter_spans(app, layout, anchor, first_of_month, line_idx);
    append_block(&mut spans, gutter, layout.gutter_width, None);
    if layout.gutter_gap > 0 {
        spans.push(spaces(usize::from(layout.gutter_gap), Style::default()));
    }

    for (i, cell) in cells.iter().enumerate() {
        let content = cell.lines.get(line_idx).cloned().unwrap_or_default();
        append_block(&mut spans, content, layout.cell_widths[i], cell.bg);
        if i < 6 {
            spans.push(separator_span(theme));
        }
    }

    render_spans(&spans, layout.width, Style::default(), theme)
}

fn build_cell(app: &App, date: NaiveDate, width: u16, height: u16) -> Cell {
    let theme = &app.theme;
    let month = date.month();
    let is_today = date == app.today;
    let bg = if theme.mode == ColorMode::Mono || theme.tint == 0 {
        None
    } else if is_today {
        Some(theme.today_bg(month))
    } else {
        Some(theme.month_bg(month))
    };

    let events: &[DisplayEvent] = if app.settings.show_events {
        app.events_for(date)
    } else {
        &[]
    };
    let capacity = usize::from(height.saturating_sub(1));
    let mut lines = Vec::with_capacity(usize::from(height));
    if height == 0 {
        return Cell { bg, lines };
    }
    lines.push(day_header_spans(app, date, width, events, capacity));
    for event in events.iter().take(capacity) {
        lines.push(event_line_spans(app, Some(event), width));
    }
    while lines.len() < usize::from(height) {
        lines.push(Vec::new());
    }
    Cell { bg, lines }
}

fn day_header_spans(
    app: &App,
    date: NaiveDate,
    width: u16,
    events: &[DisplayEvent],
    capacity: usize,
) -> Vec<Span> {
    let theme = &app.theme;
    let lang = app.settings.lang;
    let target = usize::from(width);
    let mut spans = Vec::new();
    let mut used = 0usize;

    let is_today = date == app.today;
    if target >= 3 {
        spans.push(spaces(1, Style::default()));
        used = 1;
    }

    let day_digits = date.day().to_string();
    let remaining = target.saturating_sub(used);
    let number = if remaining >= 2 {
        format!("{:>2}", date.day())
    } else if remaining == 1 {
        day_digits
            .chars()
            .next_back()
            .map(|c| c.to_string())
            .unwrap_or_default()
    } else {
        String::new()
    };
    let day_style = if is_today {
        let mut style = Style::fg(theme.today.readable_text())
            .with_bg(Some(theme.today))
            .with_bold();
        if theme.mode == ColorMode::Mono {
            style = style.with_reverse();
        }
        style
    } else {
        Style::fg(theme.day_fg(date.month(), calendar::is_weekend(date))).with_bold()
    };
    if !number.is_empty() {
        push_fit(&mut spans, &number, day_style, &mut used, target);
    }

    // En mode compact, une seule ligne peut afficher un aperçu ; le reste est
    // signalé par un « +N ». En mode multi-lignes, +N compte les événements
    // qui ne tiennent pas dans les lignes d'événements.
    let hidden = if capacity == 0 {
        events.len().saturating_sub(1)
    } else {
        events.len().saturating_sub(capacity)
    };
    let indicator = (hidden > 0 && target > used + 1).then(|| format!("+{hidden}"));
    let reserve = indicator
        .as_ref()
        .map(|text| width_of(text) + 1)
        .unwrap_or(0);

    if date.day() == 1 {
        let full = lang.months()[date.month() as usize - 1];
        let short = lang.months_short()[date.month() as usize - 1];
        let available = target.saturating_sub(used + reserve);
        let candidate = if width_of(full) < available {
            Some(full)
        } else if width_of(short) < available {
            Some(short)
        } else {
            None
        };
        if let Some(month) = candidate {
            if used < target {
                spans.push(spaces(1, Style::default()));
                used += 1;
            }
            push_fit(
                &mut spans,
                month,
                Style::fg(theme.accent_fg(date.month())).with_bold(),
                &mut used,
                target,
            );
        }
    }

    if capacity == 0 {
        // Mode compact : aperçu du premier événement, sans empiéter sur +N.
        if let Some(event) = events.first() {
            let limit = target.saturating_sub(reserve);
            if limit.saturating_sub(used) >= 3 {
                spans.push(spaces(1, Style::default()));
                used += 1;
                let marker = event_marker(event);
                push_fit(
                    &mut spans,
                    marker,
                    Style::fg(theme.source_text_fg(event.color)).with_bold(),
                    &mut used,
                    limit,
                );
                if limit.saturating_sub(used) >= 2 {
                    spans.push(spaces(1, Style::default()));
                    used += 1;
                    let title = clean_title(&event.title);
                    push_fit(
                        &mut spans,
                        &title,
                        Style::fg(theme.event_fg()),
                        &mut used,
                        limit,
                    );
                }
            }
        }
    }

    if let Some(text) = indicator {
        let text_width = width_of(&text);
        let remaining = target.saturating_sub(used);
        if remaining > text_width {
            spans.push(spaces(remaining - text_width, Style::default()));
            spans.push(Span::new(text, Style::fg(theme.muted)));
        }
    }

    let _ = lang;
    spans
}

fn event_marker(event: &DisplayEvent) -> &'static str {
    if event.continues {
        "↳"
    } else if event.all_day {
        "▪"
    } else {
        "•"
    }
}

fn event_line_spans(app: &App, event: Option<&DisplayEvent>, width: u16) -> Vec<Span> {
    let theme = &app.theme;
    let target = usize::from(width);
    let mut spans = Vec::new();
    let Some(event) = event else {
        return spans;
    };
    let mut used = 0usize;
    if target >= 3 {
        spans.push(spaces(1, Style::default()));
        used = 1;
    }

    let marker = event_marker(event);
    if !push_fit(
        &mut spans,
        marker,
        Style::fg(theme.source_text_fg(event.color)).with_bold(),
        &mut used,
        target,
    ) {
        return spans;
    }

    let timed = !event.all_day && !event.continues;
    if theme.show_event_time && timed {
        let time = event
            .start_time
            .map(|t| t.format("%H:%M").to_string())
            .unwrap_or_default();
        let time_width = width_of(&time);
        if !time.is_empty() && target.saturating_sub(used) >= time_width + 3 {
            spans.push(spaces(1, Style::default()));
            used += 1;
            push_fit(
                &mut spans,
                &time,
                Style::fg(theme.event_time_fg()),
                &mut used,
                target,
            );
            spans.push(spaces(1, Style::default()));
            used += 1;
        } else if used < target {
            spans.push(spaces(1, Style::default()));
            used += 1;
        }
    } else if used < target {
        spans.push(spaces(1, Style::default()));
        used += 1;
    }

    if used < target {
        let title = clean_title(&event.title);
        push_fit(
            &mut spans,
            &title,
            Style::fg(theme.event_fg()),
            &mut used,
            target,
        );
    }
    let _ = &theme;
    spans
}

// ─────────────────────────────────────────────────────────────────────────────
//  Pied de page
// ─────────────────────────────────────────────────────────────────────────────

fn event_phrase(lang: Lang, n: usize) -> String {
    match n {
        0 => lang.t("no_events").to_string(),
        1 => format!("1 {}", lang.t("event")),
        _ => format!("{n} {}", lang.t("events")),
    }
}

fn key_hints(lang: Lang) -> [&'static str; 3] {
    match lang {
        Lang::Fr => [
            "molette ↑↓  ·  PgUp/PgDn  ·  ←→ mois  ·  t aujourd'hui  ·  r actualiser  ·  c config  ·  i aide  ·  q quitter",
            "molette ↑↓  ·  t aujourd'hui  ·  r actualiser  ·  i aide  ·  q quitter",
            "molette·t·r·c·i·q",
        ],
        Lang::En => [
            "wheel ↑↓  ·  PgUp/PgDn  ·  ←→ month  ·  t today  ·  r refresh  ·  c config  ·  i help  ·  q quit",
            "wheel ↑↓  ·  t today  ·  r refresh  ·  i help  ·  q quit",
            "wheel·t·r·c·i·q",
        ],
    }
}

fn footer_source_variants(app: &App) -> (Vec<Span>, Vec<Span>) {
    let theme = &app.theme;
    let active: Vec<&SourceState> = app
        .sources
        .iter()
        .filter(|source| source.source.is_active())
        .collect();
    if active.is_empty() {
        return (Vec::new(), Vec::new());
    }

    let mut full = Vec::new();
    let mut compact = Vec::new();
    for (index, source) in active.iter().enumerate() {
        if index > 0 {
            full.push(spaces(2, Style::default()));
            compact.push(spaces(1, Style::default()));
        }
        let (symbol, symbol_style) = match source.status {
            SourceStatus::Loading => ("⟳", Style::fg(theme.warning()).with_bold()),
            SourceStatus::Error => ("⚠", Style::fg(theme.error()).with_bold()),
            SourceStatus::Loaded => ("●", Style::fg(source.source.color).with_bold()),
            _ => ("○", Style::fg(theme.faint)),
        };
        full.push(Span::new(symbol, symbol_style));
        full.push(spaces(1, Style::default()));
        full.push(Span::new(
            source.source.name.clone(),
            Style::fg(theme.muted),
        ));
        if source.status == SourceStatus::Loaded {
            full.push(Span::new(
                format!(" {}", source.events.len()),
                Style::fg(theme.faint),
            ));
        }

        compact.push(Span::new(symbol, symbol_style));
        if source.status == SourceStatus::Loaded {
            compact.push(Span::new(
                format!("{}", source.events.len()),
                Style::fg(theme.faint),
            ));
        }
    }
    (full, compact)
}

fn footer_message_entries(app: &App) -> Vec<(String, Style)> {
    let theme = &app.theme;
    let lang = app.settings.lang;
    let mut entries: Vec<(String, Style)> = Vec::new();

    if let Some(error) = &app.config_error {
        entries.push((format!(" ⚠ {error}"), Style::fg(theme.error()).with_bold()));
    }
    if let Some(toast) = app.current_toast() {
        let style = match toast.kind {
            ToastKind::Error => Style::fg(theme.error()).with_bold(),
            ToastKind::Warning => Style::fg(theme.warning()),
            ToastKind::Success => Style::fg(theme.success()),
            ToastKind::Info => Style::fg(theme.muted),
        };
        let symbol = match toast.kind {
            ToastKind::Error | ToastKind::Warning => " ! ",
            ToastKind::Success => " ✓ ",
            ToastKind::Info => " · ",
        };
        entries.push((format!("{symbol}{}", toast.text), style));
    }
    for source in &app.sources {
        if source.status == SourceStatus::Error
            && let Some(error) = &source.error
        {
            entries.push((
                format!(" ⚠ {} : {error}", source.source.name),
                Style::fg(theme.error()),
            ));
        }
    }
    for warning in app.config_warnings.iter().take(1) {
        entries.push((format!(" ! {warning}"), Style::fg(theme.warning())));
    }
    if entries.is_empty() && app.active_source_count() == 0 {
        entries.push((
            format!(" · {}", lang.t("no_remote")),
            Style::fg(theme.muted),
        ));
    }
    entries
}

fn compose_footer_attempt(
    keys: &str,
    sources: &[Span],
    width: u16,
    theme: &Theme,
) -> Option<String> {
    let mut spans = vec![
        spaces(1, Style::default()),
        Span::new(keys, Style::fg(theme.muted)),
    ];
    let key_width = spans_width(&spans);
    let source_width = spans_width(sources);
    let target = usize::from(width);
    if key_width > target {
        return None;
    }
    if source_width > 0 {
        if key_width + 2 + source_width > target {
            return None;
        }
        spans.push(spaces(target - key_width - source_width, Style::default()));
        spans.extend_from_slice(sources);
    }
    Some(render_spans(&spans, width, Style::default(), theme))
}

fn footer_lines(app: &App, layout: &Layout) -> Vec<String> {
    let theme = &app.theme;
    let width = layout.width;
    let message_slots = usize::from(layout.footer_height.saturating_sub(1));
    let mut messages = footer_message_entries(app);
    messages.truncate(message_slots);

    let mut lines = Vec::with_capacity(usize::from(layout.footer_height));
    let missing_messages = message_slots.saturating_sub(messages.len());
    for _ in 0..missing_messages {
        lines.push(blank_line(width, theme));
    }
    for (text, style) in messages {
        lines.push(render_spans(
            &[Span::new(text, style)],
            width,
            Style::default(),
            theme,
        ));
    }

    let (full_sources, compact_sources) = footer_source_variants(app);
    let hints = key_hints(app.settings.lang);
    let variants: [(usize, u8); 7] = [(0, 2), (1, 2), (2, 2), (0, 1), (1, 1), (2, 1), (2, 0)];
    let mut keys_line = None;
    for (hint_index, source_kind) in variants {
        let sources: &[Span] = match source_kind {
            2 => &full_sources,
            1 => &compact_sources,
            _ => &[],
        };
        if let Some(line) = compose_footer_attempt(hints[hint_index], sources, width, theme) {
            keys_line = Some(line);
            break;
        }
    }
    lines.push(keys_line.unwrap_or_else(|| {
        render_spans(
            &[
                spaces(1, Style::default()),
                Span::new(hints[2], Style::fg(theme.muted)),
            ],
            width,
            Style::default(),
            theme,
        )
    }));

    while lines.len() < usize::from(layout.footer_height) {
        lines.push(blank_line(width, theme));
    }
    lines.truncate(usize::from(layout.footer_height));
    lines
}

// ─────────────────────────────────────────────────────────────────────────────
//  Écran d'informations
// ─────────────────────────────────────────────────────────────────────────────

fn info_lines(app: &App) -> Vec<String> {
    let theme = &app.theme;
    let lang = app.settings.lang;
    let (width, height) = app.size;
    let accent = theme.accent(app.anchor.month());
    let accent_fg = theme.accent_fg(app.anchor.month());
    let bar_bg = if theme.mode == ColorMode::Mono {
        None
    } else {
        Some(theme.base.blend(accent, if theme.dark { 14 } else { 9 }))
    };

    let brand = Style::fg(accent.readable_text())
        .with_bg(Some(accent))
        .with_bold();
    let left = vec![
        Span::new(" contcal ", brand),
        spaces(2, Style::default()),
        Span::new(
            format!("{} · {VERSION}", lang.t("info")),
            Style::fg(theme.fg).with_bold(),
        ),
    ];
    let right = vec![Span::new(
        format!("{}  ", lang.t("close")),
        Style::fg(theme.faint),
    )];
    let mut out = Vec::with_capacity(usize::from(height));
    out.push(compose_bar(left, right, width, bar_bg, theme));
    out.push(render_spans(
        &[Span::new(
            "─".repeat(usize::from(width)),
            Style::fg(theme.border()),
        )],
        width,
        Style::default(),
        theme,
    ));

    let mut content: Vec<Vec<Span>> = Vec::new();

    push_heading(&mut content, theme, accent_fg, lang.t("configuration"));
    let file_label = if lang == Lang::Fr { "Fichier" } else { "File" };
    push_kv(
        &mut content,
        theme,
        file_label,
        &pretty_path(&app.settings.config_path),
        Style::fg(theme.fg),
    );
    let color_mode = match theme.mode {
        ColorMode::TrueColor => "truecolor",
        ColorMode::Ansi256 => "256 couleurs",
        ColorMode::Mono => "mono",
    };
    let effective_rows = app
        .layout
        .as_ref()
        .map(|layout| layout.row_height)
        .unwrap_or(app.settings.row_height);
    let row_label = match (lang, effective_rows) {
        (Lang::Fr, 1) => "1 ligne/semaine".to_string(),
        (Lang::Fr, n) => format!("{n} lignes/semaine"),
        (Lang::En, 1) => "1 line/week".to_string(),
        (Lang::En, n) => format!("{n} lines/week"),
    };
    let tint_label = if lang == Lang::Fr { "teinte" } else { "tint" };
    push_kv(
        &mut content,
        theme,
        lang.t("terminal"),
        &format!(
            "{}×{} · {} · {} · {} {} %",
            width, height, color_mode, row_label, tint_label, app.settings.month_tint
        ),
        Style::fg(theme.fg),
    );
    if let Some(layout) = app.layout.as_ref() {
        push_kv(
            &mut content,
            theme,
            lang.t("view"),
            &format!(
                "{} · {}",
                calendar::range_label(lang, app.first_visible(), app.last_visible()),
                calendar::weeks_phrase(lang, u64::from(layout.weeks))
            ),
            Style::fg(theme.fg),
        );
        let events = if app.settings.show_events {
            match lang {
                Lang::Fr => format!(
                    "{} visibles · {} chargés",
                    app.visible_event_count(),
                    app.total_event_count()
                ),
                Lang::En => format!(
                    "{} visible · {} loaded",
                    app.visible_event_count(),
                    app.total_event_count()
                ),
            }
        } else if lang == Lang::Fr {
            "masqués".to_string()
        } else {
            "hidden".to_string()
        };
        let events_label = if lang == Lang::Fr {
            "Événements"
        } else {
            "Events"
        };
        push_kv(
            &mut content,
            theme,
            events_label,
            &events,
            Style::fg(theme.fg),
        );
    }
    let palette_label = if lang == Lang::Fr {
        "12 couleurs de mois"
    } else {
        "12 month colours"
    };
    let weekend_label = if app.settings.weekend_dim {
        if lang == Lang::Fr {
            "week-end atténué"
        } else {
            "weekends dimmed"
        }
    } else if lang == Lang::Fr {
        "week-end normal"
    } else {
        "weekends normal"
    };
    push_kv(
        &mut content,
        theme,
        lang.t("theme"),
        &format!(
            "{} · {} · {}",
            if theme.dark {
                lang.t("dark")
            } else {
                lang.t("light")
            },
            palette_label,
            weekend_label
        ),
        Style::fg(theme.fg),
    );

    if let Some(error) = &app.config_error {
        push_wrapped(
            &mut content,
            theme,
            &format!("⚠ {error}"),
            Style::fg(theme.error()).with_bold(),
            2,
            usize::from(width).saturating_sub(2),
        );
    }

    content.push(Vec::new());
    push_heading(&mut content, theme, accent_fg, lang.t("calendars"));
    if app.sources.is_empty() {
        push_wrapped(
            &mut content,
            theme,
            lang.t("no_remote"),
            Style::fg(theme.warning()),
            2,
            usize::from(width).saturating_sub(2),
        );
    } else {
        for source in &app.sources {
            push_source_info(&mut content, source, theme, lang, width);
        }
    }

    if !app.config_warnings.is_empty() {
        content.push(Vec::new());
        push_heading(
            &mut content,
            theme,
            accent_fg,
            if lang == Lang::Fr {
                "Avertissements"
            } else {
                "Warnings"
            },
        );
        for warning in app.config_warnings.iter().take(5) {
            push_wrapped(
                &mut content,
                theme,
                &format!("! {warning}"),
                Style::fg(theme.warning()),
                2,
                usize::from(width).saturating_sub(2),
            );
        }
    }

    content.push(Vec::new());
    push_heading(&mut content, theme, accent_fg, lang.t("shortcuts"));
    for (key, desc) in shortcuts(lang) {
        content.push(vec![
            spaces(2, Style::default()),
            Span::new(pad_right(key, 17), Style::fg(theme.fg).with_bold()),
            Span::new(desc, Style::fg(theme.muted)),
        ]);
    }

    let capacity = usize::from(height).saturating_sub(3);
    if content.len() > capacity {
        content.truncate(capacity);
        if let Some(last) = content.last_mut() {
            *last = vec![
                spaces(2, Style::default()),
                Span::new("…", Style::fg(theme.faint)),
            ];
        }
    }
    while content.len() < capacity {
        content.push(Vec::new());
    }
    for line in content {
        out.push(render_spans(&line, width, Style::default(), theme));
    }

    let footer_left = match lang {
        Lang::Fr => " Échap pour fermer  ·  r actualiser",
        Lang::En => " Esc to close  ·  r refresh",
    };
    let footer_right = format!(
        "{}  ·  contcal {VERSION} ",
        pretty_path(&app.settings.config_path)
    );
    let mut footer_spans = vec![Span::new(footer_left, Style::fg(theme.faint))];
    let left_width = spans_width(&footer_spans);
    let right_spans = vec![Span::new(footer_right, Style::fg(theme.faint))];
    let right_width = spans_width(&right_spans);
    let target = usize::from(width);
    if left_width + 2 + right_width <= target {
        footer_spans.push(spaces(target - left_width - right_width, Style::default()));
        footer_spans.extend(right_spans);
    }
    out.push(render_spans(&footer_spans, width, Style::default(), theme));

    while out.len() < usize::from(height) {
        out.push(blank_line(width, theme));
    }
    out.truncate(usize::from(height));
    out
}

fn push_heading(content: &mut Vec<Vec<Span>>, theme: &Theme, accent: Rgb, text: &str) {
    content.push(vec![
        spaces(2, Style::default()),
        Span::new(text, Style::fg(accent).with_bold().with_underline()),
    ]);
    let _ = theme;
}

fn push_kv(
    content: &mut Vec<Vec<Span>>,
    theme: &Theme,
    label: &str,
    value: &str,
    value_style: Style,
) {
    content.push(vec![
        spaces(2, Style::default()),
        Span::new(pad_right(label, 13), Style::fg(theme.faint)),
        Span::new(value, value_style),
    ]);
}

fn push_source_info(
    content: &mut Vec<Vec<Span>>,
    source: &SourceState,
    theme: &Theme,
    lang: Lang,
    width: u16,
) {
    let (symbol, symbol_style) = match source.status {
        SourceStatus::Error => ("⚠", Style::fg(theme.error()).with_bold()),
        SourceStatus::Loading => ("⟳", Style::fg(theme.warning()).with_bold()),
        SourceStatus::Loaded => ("●", Style::fg(source.source.color).with_bold()),
        SourceStatus::MissingUrl => ("!", Style::fg(theme.warning()).with_bold()),
        SourceStatus::Disabled => ("○", Style::fg(theme.faint)),
        SourceStatus::Idle => ("○", Style::fg(theme.faint)),
    };
    let description = match source.status {
        SourceStatus::Disabled => lang.t("disabled").to_string(),
        SourceStatus::MissingUrl => lang.t("url_missing").to_string(),
        SourceStatus::Idle => {
            if lang == Lang::Fr {
                "en attente".into()
            } else {
                "waiting".into()
            }
        }
        SourceStatus::Loading => lang.t("loading").to_string(),
        SourceStatus::Loaded => {
            let age = source
                .last_success
                .map(|t| age_phrase(lang, t.elapsed()))
                .unwrap_or_else(|| lang.t("never").to_string());
            format!("{} · {age}", event_phrase(lang, source.events.len()))
        }
        SourceStatus::Error => {
            if let Some(t) = source.last_success {
                format!("{} · {}", lang.t("error"), age_phrase(lang, t.elapsed()))
            } else {
                lang.t("error").to_string()
            }
        }
    };
    content.push(vec![
        spaces(2, Style::default()),
        Span::new(symbol, symbol_style),
        spaces(1, Style::default()),
        Span::new(source.source.name.clone(), Style::fg(theme.fg).with_bold()),
        spaces(2, Style::default()),
        Span::new(description, Style::fg(theme.muted)),
    ]);

    let url = fetch::masked_url(&source.source.url);
    let url_style = if source.source.url.trim().is_empty() {
        Style::fg(theme.warning())
    } else {
        Style::fg(theme.faint)
    };
    content.push(vec![
        spaces(4, Style::default()),
        Span::new("URL  ", Style::fg(theme.faint)),
        Span::new(url, url_style),
    ]);

    if let Some(error) = &source.error {
        push_wrapped(
            content,
            theme,
            &format!("⚠ {error}"),
            Style::fg(theme.error()),
            4,
            usize::from(width).saturating_sub(4),
        );
    }
    if !source.warnings.is_empty() {
        let first = &source.warnings[0];
        let more = source.warnings.len().saturating_sub(1);
        let text = if more > 0 {
            format!("! {first} (+{more})")
        } else {
            format!("! {first}")
        };
        push_wrapped(
            content,
            theme,
            &text,
            Style::fg(theme.warning()),
            4,
            usize::from(width).saturating_sub(4),
        );
    }
    content.push(Vec::new());
}

fn push_wrapped(
    content: &mut Vec<Vec<Span>>,
    theme: &Theme,
    text: &str,
    style: Style,
    indent: usize,
    width: usize,
) {
    let avail = width.saturating_sub(indent).max(1);
    for line in wrap_text(text, avail) {
        content.push(vec![
            spaces(indent, Style::default()),
            Span::new(line, style),
        ]);
    }
    let _ = theme;
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let cleaned = sanitize(text);
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;

    for word in cleaned.split_whitespace() {
        let word_width = width_of(word);
        if word_width > width {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
                current_width = 0;
            }
            let mut chunk = String::new();
            let mut chunk_width = 0usize;
            for ch in word.chars() {
                let cw = ch.width().unwrap_or(0);
                if chunk_width + cw > width {
                    lines.push(std::mem::take(&mut chunk));
                    chunk_width = 0;
                }
                chunk.push(ch);
                chunk_width += cw;
            }
            if !chunk.is_empty() {
                lines.push(chunk);
            }
            continue;
        }
        if current.is_empty() {
            current.push_str(word);
            current_width = word_width;
        } else if current_width + 1 + word_width <= width {
            current.push(' ');
            current.push_str(word);
            current_width += 1 + word_width;
        } else {
            lines.push(std::mem::take(&mut current));
            current.push_str(word);
            current_width = word_width;
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn age_phrase(lang: Lang, elapsed: std::time::Duration) -> String {
    let seconds = elapsed.as_secs();
    match lang {
        Lang::Fr => {
            if seconds < 60 {
                "à l'instant".into()
            } else if seconds < 3600 {
                format!("il y a {} min", seconds / 60)
            } else if seconds < 86_400 {
                format!("il y a {} h", seconds / 3600)
            } else {
                format!("il y a {} j", seconds / 86_400)
            }
        }
        Lang::En => {
            if seconds < 60 {
                "just now".into()
            } else if seconds < 3600 {
                format!("{} min ago", seconds / 60)
            } else if seconds < 86_400 {
                format!("{} h ago", seconds / 3600)
            } else {
                format!("{} d ago", seconds / 86_400)
            }
        }
    }
}

fn shortcuts(lang: Lang) -> [(&'static str, &'static str); 8] {
    match lang {
        Lang::Fr => [
            ("molette / ↑↓", "défiler d'une semaine"),
            ("PgUp / PgDn", "défiler d'une page"),
            ("← / →", "mois précédent / suivant"),
            ("t / Origine", "revenir à aujourd'hui"),
            ("r", "actualiser les calendriers"),
            ("c", "recharger la configuration"),
            ("i / ?", "cette fenêtre"),
            ("q", "quitter"),
        ],
        Lang::En => [
            ("wheel / ↑↓", "scroll one week"),
            ("PgUp / PgDn", "scroll one page"),
            ("← / →", "previous / next month"),
            ("t / Home", "back to today"),
            ("r", "refresh calendars"),
            ("c", "reload configuration"),
            ("i / ?", "this window"),
            ("q", "quit"),
        ],
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Terminal trop petit
// ─────────────────────────────────────────────────────────────────────────────

fn too_small_lines(app: &App) -> Vec<String> {
    let theme = &app.theme;
    let lang = app.settings.lang;
    let (width, height) = app.size;
    let width = width.max(1);
    let height = height.max(1);
    let mut lines = Vec::with_capacity(usize::from(height));

    let message = if width < MIN_WIDTH {
        lang.t("min_cols").to_string()
    } else if height < MIN_HEIGHT {
        lang.t("min_rows").to_string()
    } else {
        lang.t("min_cols").to_string()
    };
    let size = format!("{width}×{height}");
    let quit = if lang == Lang::Fr {
        "q : quitter"
    } else {
        "q : quit"
    };
    let available = usize::from(width).saturating_sub(2).max(1);
    let mut texts = Vec::new();
    for line in wrap_text(&message, available) {
        texts.push(line);
    }
    texts.push(size);
    texts.push(quit.to_string());

    let start = usize::from(height)
        .saturating_sub(texts.len())
        .saturating_div(2);
    let mut rendered = std::collections::VecDeque::from(texts);
    let mut message_index = 0usize;
    for row in 0..usize::from(height) {
        if row >= start && !rendered.is_empty() {
            let text = rendered.pop_front().unwrap_or_default();
            let text_width = width_of(&text);
            let total_width = usize::from(width);
            let left = total_width.saturating_sub(text_width) / 2;
            let right = total_width.saturating_sub(left + text_width);
            let mut spans = vec![spaces(left, Style::default())];
            spans.push(Span::new(
                text,
                Style::fg(if message_index == 0 {
                    theme.warning()
                } else {
                    theme.fg
                })
                .with_bold(),
            ));
            spans.push(spaces(right, Style::default()));
            lines.push(render_spans(&spans, width, Style::default(), theme));
            message_index += 1;
        } else {
            lines.push(blank_line(width, theme));
        }
    }
    lines
}

// ─────────────────────────────────────────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::config::{self, LoadedConfig};
    use std::path::PathBuf;

    fn strip_ansi(text: &str) -> String {
        let mut out = String::new();
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '\x1b' {
                if chars.peek() == Some(&'[') {
                    chars.next();
                    while let Some(&next) = chars.peek() {
                        chars.next();
                        if ('@'..='~').contains(&next) {
                            break;
                        }
                    }
                }
            } else {
                out.push(ch);
            }
        }
        out
    }

    fn test_app_with_rows(width: u16, height: u16, rows: i64) -> App {
        let mut cfg: config::Config = toml::from_str(config::DEFAULT_CONFIG).unwrap();
        cfg.appearance.row_height = rows;
        let path = PathBuf::from("/tmp/contcal-ui-test.toml");
        let settings = cfg.to_settings(path.clone());
        let loaded = LoadedConfig {
            settings,
            warnings: Vec::new(),
            created: false,
            error: None,
            path,
        };
        let mut app = App::new(loaded, None, width, height);
        app.ensure_cache();
        app
    }

    fn test_app(width: u16, height: u16) -> App {
        test_app_with_rows(width, height, 2)
    }

    fn sample_event(
        uid: &str,
        title: &str,
        start: chrono::NaiveDate,
        all_day: bool,
    ) -> crate::ics::Event {
        crate::ics::Event {
            uid: uid.into(),
            summary: title.into(),
            location: None,
            start,
            start_time: (!all_day).then(|| chrono::NaiveTime::from_hms_opt(9, 0, 0).unwrap()),
            span_days: 1,
            all_day,
            rrule: None,
            exdates: Default::default(),
            rdates: Vec::new(),
            recurrence_id: None,
        }
    }

    #[test]
    fn every_line_fills_the_terminal_width() {
        for (width, height) in [(80, 24), (100, 40), (28, 6), (160, 50)] {
            let app = test_app(width, height);
            let lines = render(&app);
            assert_eq!(lines.len(), usize::from(height));
            for line in &lines {
                let plain = strip_ansi(line);
                assert_eq!(
                    width_of(&plain),
                    usize::from(width),
                    "largeur incorrecte pour {width}x{height}"
                );
            }
        }
    }

    #[test]
    fn events_and_hidden_counts_render() {
        for rows in [1, 2] {
            let mut app = test_app_with_rows(110, 40, rows);
            let today = app.today;
            app.sources[0].status = crate::app::SourceStatus::Loaded;
            app.sources[0].events = vec![
                sample_event("a", "Journée test", today, true),
                sample_event("b", "Réunion importante", today, false),
            ];
            app.data_generation = app.data_generation.wrapping_add(1);
            app.ensure_cache();

            let lines = render(&app);
            assert_eq!(lines.len(), 40);
            for line in &lines {
                assert_eq!(width_of(&strip_ansi(line)), 110);
            }
            let all = lines
                .iter()
                .map(|line| strip_ansi(line))
                .collect::<Vec<_>>()
                .join("\n");
            assert!(all.contains("Jour"), "rows={rows}");
            assert!(all.contains("+1"), "rows={rows}");
        }
    }

    #[test]
    fn info_screen_fills_space() {
        let mut app = test_app(100, 40);
        app.info_open = true;
        let lines = render(&app);
        assert_eq!(lines.len(), 40);
        for line in &lines {
            assert_eq!(width_of(&strip_ansi(line)), 100);
        }
        let all = lines
            .iter()
            .map(|l| strip_ansi(l))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(all.contains("Calendriers distants"));
        assert!(all.contains("Raccourcis"));
    }

    #[test]
    fn tiny_terminal_gets_a_message() {
        let app = test_app(20, 10);
        let lines = render(&app);
        assert_eq!(lines.len(), 10);
        let plain = lines
            .iter()
            .map(|l| strip_ansi(l))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(plain.contains("28"));
    }

    #[test]
    fn truncation_and_padding() {
        assert_eq!(limit_width("abcdef", 4), "abc…");
        assert_eq!(limit_width("abc", 4), "abc");
        assert_eq!(limit_width("abc", 0), "");
        assert_eq!(pad_right("ab", 4), "ab  ");
        assert_eq!(width_of(&limit_width("日本語", 3)), 3);
    }

    #[test]
    fn wrapping_respects_width() {
        let lines = wrap_text("un deux trois quatre cinq six", 10);
        assert!(lines.iter().all(|l| width_of(l) <= 10));
        assert_eq!(lines.join(" "), "un deux trois quatre cinq six");
    }

    #[test]
    fn control_characters_are_neutralized() {
        let span = Span::new("a\u{1b}[31mb\nc", Style::default());
        assert!(!span.text.contains('\u{1b}'));
        assert!(!span.text.contains('\n'));
    }
}
