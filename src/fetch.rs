//! Téléchargement des flux iCalendar distants, hors du thread d'interface.

use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, Instant};

use crate::config::Source;
use crate::ics::{self, Event};

const MAX_BODY_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug)]
pub struct FetchedCalendar {
    pub events: Vec<Event>,
    pub warnings: Vec<String>,
    pub fetched_at: Instant,
}

#[derive(Debug)]
pub struct FetchResult {
    pub source_key: String,
    pub generation: u64,
    pub outcome: Result<FetchedCalendar, String>,
}

/// Lance un téléchargement en arrière-plan. Le résultat arrive sur `tx`.
pub fn spawn(source: Source, generation: u64, timeout: Duration, tx: Sender<FetchResult>) {
    let tx_error = tx.clone();
    let key = source.key.clone();
    let thread_key = key.clone();
    let spawned = thread::Builder::new()
        .name(format!("contcal-fetch-{key}"))
        .spawn(move || {
            let outcome = download(&source, timeout);
            let _ = tx.send(FetchResult {
                source_key: thread_key,
                generation,
                outcome,
            });
        });
    if let Err(e) = spawned {
        let _ = tx_error.send(FetchResult {
            source_key: key,
            generation,
            outcome: Err(format!("impossible de lancer le téléchargement : {e}")),
        });
    }
}

fn download(source: &Source, timeout: Duration) -> Result<FetchedCalendar, String> {
    let url = source.url.trim();
    if url.is_empty() {
        return Err("URL vide".into());
    }
    let lower = url.to_ascii_lowercase();
    if lower.contains("calendar.google.com/calendar/u/")
        || (lower.contains("calendar.google.com/calendar?") && lower.contains("cid="))
    {
        return Err(
            "lien web Google Calendar (cid) : ce n'est pas un flux iCal. Utilisez l'adresse secrète au format iCal (…/basic.ics)."
                .into(),
        );
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .user_agent(concat!("contcal/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();

    let mut response = agent
        .get(url)
        .header(
            "Accept",
            "text/calendar, text/plain;q=0.9, application/octet-stream;q=0.8, */*;q=0.5",
        )
        .call()
        .map_err(describe_error)?;

    let status = response.status();
    if !status.is_success() {
        return Err(format!("HTTP {}", status.as_u16()));
    }

    let text = response
        .body_mut()
        .with_config()
        .limit(MAX_BODY_BYTES)
        .lossy_utf8(true)
        .read_to_string()
        .map_err(|e| {
            if matches!(e, ureq::Error::BodyExceedsLimit(_)) {
                "calendrier trop volumineux (16 Mo maximum)".to_string()
            } else {
                format!("lecture de la réponse impossible : {e}")
            }
        })?;

    if !ics::looks_like_ics(&text) {
        let probe = text
            .trim_start_matches('\u{feff}')
            .trim_start()
            .chars()
            .take(400)
            .collect::<String>()
            .to_ascii_lowercase();
        return Err(
            if probe.starts_with("<!doctype") || probe.starts_with("<html") {
                "réponse HTML : lien de partage invalide, expiré ou accès refusé".into()
            } else if probe.is_empty() {
                "réponse vide : lien de partage invalide ?".into()
            } else {
                "le contenu reçu n'est pas un calendrier iCalendar (.ics)".into()
            },
        );
    }

    let parsed = ics::parse_ics(&text);
    Ok(FetchedCalendar {
        events: parsed.events,
        warnings: parsed.warnings,
        fetched_at: Instant::now(),
    })
}

fn describe_error(err: ureq::Error) -> String {
    use ureq::Error;
    match &err {
        Error::StatusCode(code) => match code {
            401 | 403 => {
                format!("accès refusé (HTTP {code}) : le lien de partage n'est plus public")
            }
            404 => "calendrier introuvable (HTTP 404) : vérifiez l'URL".into(),
            410 => "lien de partage expiré (HTTP 410)".into(),
            _ => format!("HTTP {code}"),
        },
        Error::Timeout(_) => "délai de téléchargement dépassé".into(),
        Error::HostNotFound => "hôte introuvable : vérifiez l'URL et le réseau".into(),
        Error::Tls(message) => format!("erreur TLS : {message}"),
        Error::TooManyRedirects => "trop de redirections HTTP".into(),
        Error::BadUri(uri) => format!("URL invalide : {uri}"),
        Error::Io(io) => format!("erreur réseau : {io}"),
        Error::BodyExceedsLimit(_) => "calendrier trop volumineux (16 Mo maximum)".into(),
        _ => format!("téléchargement impossible : {err}"),
    }
}

/// Raccourcit une URL pour l'écran d'informations, sans exposer le jeton complet.
pub fn masked_url(url: &str) -> String {
    let url = url.trim();
    if url.is_empty() {
        return "—".into();
    }
    let (scheme, rest) = match url.split_once("://") {
        Some((s, r)) => (s, r),
        None => ("", url),
    };
    let host_end = rest.find('/').unwrap_or(rest.len());
    let host = &rest[..host_end];
    let path = &rest[host_end..];
    let last = path
        .rsplit('/')
        .find(|segment| !segment.is_empty())
        .unwrap_or("");
    let last = last.split('?').next().unwrap_or(last);
    let last = if last.to_ascii_lowercase().ends_with(".ics") {
        last
    } else {
        "…"
    };
    let shown = if host.is_empty() {
        url.to_string()
    } else if last.is_empty() {
        host.to_string()
    } else {
        format!("{host}/…/{last}")
    };
    let limit = 64usize;
    let clipped = if shown.chars().count() > limit {
        format!("{}…", shown.chars().take(limit - 1).collect::<String>())
    } else {
        shown
    };
    if scheme.is_empty() {
        clipped
    } else {
        format!("{scheme}://{clipped}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_google_and_outlook_urls() {
        let g = masked_url(
            "https://calendar.google.com/calendar/ical/abc%40gmail.com/public/basic.ics",
        );
        assert!(g.contains("calendar.google.com"));
        assert!(g.contains("basic.ics"));
        assert!(!g.contains("abc%40gmail.com"));
        assert_eq!(masked_url(""), "—");
        let o = masked_url("https://outlook.office365.com/owa/calendar/secret/token/calendar.ics");
        assert!(o.contains("outlook.office365.com"));
        assert!(o.contains("calendar.ics"));
        assert!(!o.contains("secret"));
    }

    #[test]
    fn masks_non_https_scheme() {
        assert!(masked_url("webcal://example.com/a/b.ics").starts_with("webcal://"));
    }
}
