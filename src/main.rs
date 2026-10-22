//! contcal — calendrier continu pour le terminal.

mod app;
mod calendar;
mod color;
mod config;
mod fetch;
mod ics;
mod lang;
mod layout;
mod theme;
mod ui;

use std::error::Error;
use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crossterm::cursor::{Hide, Show};
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};

use app::App;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Vrai entre l'entrée en écran alterné et la restauration.
static TERMINAL_ACTIVE: AtomicBool = AtomicBool::new(false);

struct Args {
    config: Option<PathBuf>,
    help: bool,
    version: bool,
    show_path: bool,
}

fn main() {
    let main_thread = std::thread::current().id();
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Les threads de téléchargement ne doivent pas toucher au terminal.
        if std::thread::current().id() == main_thread {
            restore_terminal();
        }
        previous_hook(info);
    }));

    if let Err(error) = run() {
        restore_terminal();
        eprintln!("contcal : {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args = parse_args()?;

    if args.help {
        print_help();
        return Ok(());
    }
    if args.version {
        println!("contcal {VERSION}");
        return Ok(());
    }
    if args.show_path {
        println!("{}", config::config_path(args.config.as_deref()).display());
        return Ok(());
    }

    if !io::stdout().is_terminal() {
        return Err("lancez contcal dans un terminal interactif (TUI).".into());
    }

    let loaded = config::load(args.config.as_deref());
    let (width, height) = terminal::size().unwrap_or((80, 24));
    let mut app = App::new(loaded, args.config, width, height);
    let _guard = TerminalGuard::enter()?;
    // Premier chargement même quand le rafraîchissement automatique est à 0.
    app.refresh();

    loop {
        app.tick();
        if app.dirty {
            let lines = ui::render(&app);
            ui::draw(&lines)?;
            app.dirty = false;
        }
        if app.quit {
            break;
        }

        if event::poll(Duration::from_millis(200))? {
            match event::read()? {
                Event::Key(key) => {
                    app.handle_key(key);
                    app.dirty = true;
                }
                Event::Mouse(mouse) => {
                    app.handle_mouse(mouse);
                    app.dirty = true;
                }
                Event::Resize(width, height) => {
                    app.resize(width, height);
                    app.dirty = true;
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn parse_args() -> Result<Args, io::Error> {
    let mut args = Args {
        config: None,
        help: false,
        version: false,
        show_path: false,
    };
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-h" | "--help" => args.help = true,
            "-V" | "--version" => args.version = true,
            "--where" => args.show_path = true,
            "-c" | "--config" => {
                let value = iter.next().ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "l'option --config attend un chemin de fichier",
                    )
                })?;
                args.config = Some(PathBuf::from(value));
            }
            other if other.starts_with("--config=") => {
                let value = &other["--config=".len()..];
                if value.is_empty() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "l'option --config attend un chemin de fichier",
                    ));
                }
                args.config = Some(PathBuf::from(value));
            }
            other if other.starts_with('-') => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("option inconnue : {other} (voir --help)"),
                ));
            }
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("argument inattendu : {other} (voir --help)"),
                ));
            }
        }
    }
    Ok(args)
}

fn print_help() {
    println!(
        "contcal {VERSION} — calendrier continu pour le terminal

UTILISATION
  contcal [OPTIONS]

OPTIONS
  -c, --config <CHEMIN>   fichier de configuration
                          (défaut : ~/.contcal.toml, ou $CONTCAL_CONFIG)
      --where             affiche le chemin du fichier de configuration
  -h, --help              affiche cette aide
  -V, --version           affiche la version

CONFIGURATION
  Le fichier est créé au premier lancement, avec des commentaires et des
  exemples. Renseignez les URL de partage iCalendar (.ics) des calendriers :

    [calendars.outlook]
    enabled = true
    url = \"https://outlook.office365.com/owa/calendar/…/calendar.ics\"

    [calendars.google]
    enabled = true
    url = \"https://calendar.google.com/calendar/ical/…/basic.ics\"

  Les liens « webcal:// » sont acceptés. D'autres tables [calendars.<nom>]
  permettent de suivre plusieurs calendriers.

NAVIGATION
  molette / ↑ ↓         défiler d'une semaine
  PgUp / PgDn           défiler d'une page
  ← / →                 mois précédent / suivant
  t / Home              revenir à aujourd'hui
  r                     actualiser les calendriers
  c                     recharger la configuration
  i / ?                 écran d'informations
  g / G                 début / fin de la plage (1900 / 2200)
  q / Échap, Ctrl+C     quitter

  Les heures UTC (« Z ») sont converties dans le fuseau local ; les heures
  portant un TZID sont affichées telles quelles."
    );
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let mut stdout = io::stdout();
        let result = execute!(stdout, EnterAlternateScreen, EnableMouseCapture, Hide);
        if let Err(error) = result {
            let _ = terminal::disable_raw_mode();
            return Err(error);
        }
        TERMINAL_ACTIVE.store(true, Ordering::SeqCst);
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

fn restore_terminal() {
    if !TERMINAL_ACTIVE.swap(false, Ordering::SeqCst) {
        return;
    }
    let _ = terminal::disable_raw_mode();
    let mut stdout = io::stdout();
    let _ = execute!(stdout, DisableMouseCapture, Show, LeaveAlternateScreen);
}
