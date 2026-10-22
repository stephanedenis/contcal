# contcal

Un calendrier continu pour le terminal, en Rust : une semaine par rangée, du
lundi au dimanche, sans rupture aux changements de mois. Les mois changent de
couleur (bandes de fond et teintes de texte), la grille occupe toute la largeur
et toute la hauteur du terminal, et la molette parcourt le temps sans limite
raisonnable (1900 → 2200).

Les calendriers partagés Outlook / Microsoft 365 et Google Calendar sont lus
depuis leurs flux iCalendar (`.ics`), et les événements apparaissent directement
dans les cases.

```text
 contcal   juillet – octobre 2026                         ◉ jeu 22 octobre  ·  18 événements
 Sem   LUNDI          MARDI          MERCREDI       JEUDI          VENDREDI       SAMEDI         DIMANCHE
 S30   20             21             22             23             24             25             26
 S31   27             28             29             30             31              1 août         2
 S32    3              4              5              6              7              8              9
 ...
 S43   19             20             21 ▪ Journée…  22 ▪ Fête… +1  23 • Revue d…  24 • Brunch    25
 S44   26 • Standup   27             28 • Standup   29 • Concert   30             31              1 novembre
```

## Fonctionnalités

- Calendrier continu : les mois se succèdent sans interruption, une semaine par
  rangée (une seule ligne par défaut), alignée sur le lundi par défaut.
- Couleur propre à chaque mois, appliquée à chaque case : une semaine à cheval
  sur deux mois garde sa continuité et change de couleur au fil des colonnes.
- Mise en page adaptative : largeur et hauteur exactes, redimensionnement du
  terminal, jour courant centré à l'ouverture. Si la hauteur ne permet pas un
  nombre entier de semaines, la dernière rangée est partielle : le pied de page
  reste compact et la grille remplit tout l'écran.
- Défilement molette (et clavier), rafraîchissement des flux en arrière-plan.
- Événements affichés le jour concerné, avec heure de début, événements
  « journée entière », événements sur plusieurs jours et récurrences courantes.
- Numéros de semaine ISO, nom du mois à côté du 1er (et dans la gouttière en
  mode multi-lignes), jour courant mis en évidence.
- Écran d'informations (`i`) : configuration, état des calendriers, erreurs
  détaillées et raccourcis.
- Configuration unique dans le home, rechargée à chaud avec `c`.

## Installation

Prérequis : Rust 1.88 ou plus récent (édition 2024) et un terminal UTF-8.

```bash
cargo build --release
./target/release/contcal
```

Ou pour l'installer dans le `PATH` de Cargo :

```bash
cargo install --path .
```

## Configuration

Au premier lancement, `contcal` crée `~/.contcal.toml` (mode `0600`) avec toutes
les options commentées. Le chemin peut être changé avec :

```bash
contcal --config ~/Documents/mon-calendrier.toml
contcal --where                 # affiche le chemin utilisé
CONTCAL_CONFIG=... contcal      # variable d'environnement
```

### Calendriers Outlook et Google

Renseignez les URL de partage iCalendar dans les tables prévues :

```toml
[calendars.outlook]
enabled = true
name = "Outlook"
url = "https://outlook.office365.com/owa/calendar/<id>/<jeton>/calendar.ics"
# color = "#4ea1ff"

[calendars.google]
enabled = true
name = "Google"
url = "https://calendar.google.com/calendar/ical/<id>/public/basic.ics"
# color = "#ff6b6b"
```

**Outlook / Microsoft 365** : ouvrez Outlook sur le web → *Calendrier* →
*Partager* → *Publier un calendrier*, puis copiez le lien `.ics`. Le lien doit
être accessible sans authentification (calendrier publié).

**Google Calendar** : *Paramètres du calendrier* → *Intégrer le calendrier* →
*Adresse secrète au format iCal* (ou l'adresse publique `.ics`). Copiez l'URL
fournie.

Les liens `webcal://` sont acceptés et convertis en `https://`. D'autres
calendriers peuvent être ajoutés avec des tables `[calendars.<nom>]`
supplémentaires. Les URL de partage contiennent souvent un jeton secret :
gardez le fichier privé.

### Options

| Clé | Défaut | Description |
| --- | --- | --- |
| `calendar.week_start` | `"monday"` | `"monday"` ou `"sunday"` |
| `calendar.start` | `"today"` | `"today"` ou `"AAAA-MM-JJ"` (rangée du haut) |
| `calendar.week_numbers` | `true` | Numéros de semaine ISO dans la gouttière |
| `calendar.language` | `"fr"` | `"fr"` ou `"en"` |
| `appearance.row_height` | `1` | Lignes par semaine : 1 (une rangée) à 8 |
| `appearance.scroll_weeks` | `1` | Semaines par cran de molette |
| `appearance.month_tint` | `16` | Intensité du fond coloré, 0 à 45 % (0 = aucun fond) |
| `appearance.color_mode` | `"auto"` | `auto`, `truecolor`, `256` ou `mono` |
| `appearance.background` | `"auto"` | `auto`, `dark` ou `light` |
| `appearance.show_events` | `true` | Afficher les événements |
| `appearance.show_event_time` | `true` | Heure de début devant le titre (mode multi-lignes) |
| `appearance.weekend_dim` | `true` | Atténuer samedi et dimanche |
| `events.refresh_minutes` | `30` | Rafraîchissement auto (0 = manuel avec `r`) |
| `events.timeout_seconds` | `20` | Délai max. d'un téléchargement |
| `colors.today` | `"#f5c542"` | Couleur du jour courant |
| `colors.palette` | *(interne)* | 12 couleurs hexadécimales, janvier → décembre |
| `calendars.<nom>.url` | `""` | URL du flux ICS |
| `calendars.<nom>.enabled` | `true` | Active ou ignore la source |
| `calendars.<nom>.name` | clé | Nom affiché |
| `calendars.<nom>.color` | selon la source | Couleur de la puce des événements |

Le premier chargement des flux a lieu même si `refresh_minutes = 0` ; ensuite,
utilisez `r` pour actualiser.

## Raccourcis

| Touche | Action |
| --- | --- |
| molette, `↑` / `↓`, `k` / `j` | Défiler d'une semaine |
| `PageUp` / `PageDown` | Défiler d'une page |
| `←` / `→`, `h` / `l` | Mois précédent / suivant |
| `t` ou `Home` | Revenir à aujourd'hui (semaine centrée) |
| `r` | Actualiser les calendriers |
| `c` | Recharger la configuration |
| `i`, `?` ou `F1` | Écran d'informations |
| `g` / `G` | Aller aux bornes 1900 / 2200 |
| `q` ou `Échap` | Quitter |
| `Ctrl+C` | Quitter |

Dans l'écran d'informations, `Échap` ferme la fenêtre, `q` quitte, et `r` / `c`
actualisent les calendriers ou la configuration.

## Formats pris en charge

- iCalendar / RFC 5545 : `VEVENT`, `SUMMARY`, `DTSTART`, `DTEND`, `DURATION`,
  `EXDATE`, `RDATE`, `RECURRENCE-ID`, `STATUS:CANCELLED`.
- Événements sur plusieurs jours et événements « journée entière ».
- Récurrences : `FREQ` `DAILY` / `WEEKLY` / `MONTHLY` / `YEARLY`, `INTERVAL`,
  `COUNT`, `UNTIL`, `BYDAY` (dont `2TU`, `-1FR`), `BYMONTHDAY`, `BYMONTH`,
  `WKST`.
- Heures `Z` converties dans le fuseau local ; heures flottantes et `TZID`
  affichées selon leur heure murale (sans base complète des fuseaux horaires).
- Téléchargement HTTP/HTTPS uniquement, corps limité à 16 Mo, redirections
  suivies, proxy d'environnement pris en charge.

L'application est en lecture seule : elle ne modifie jamais les calendriers.
Aucune authentification OAuth n'est gérée ; utilisez les liens de partage
publics ou secrets fournis par Outlook et Google.

## Limites et dépannage

- Terminal minimal : 28 colonnes × 6 lignes. En dessous, un message s'affiche.
- Bornes de navigation : 1900-01-01 → 2200-12-31.
- Couleurs : sur un terminal sans 256 couleurs (ou avec `NO_COLOR` / mode
  `mono`), les fonds colorés sont désactivés ; les noms de mois et la gouttière
  restent visibles.
- « réponse HTML : lien de partage invalide, expiré ou accès refusé » : l'URL
  pointe vers une page de connexion ou n'est plus publique.
- « HTTP 401/403/404 » : le lien a expiré, a été révoqué ou comporte une faute.
- Un lien `calendar.google.com/calendar/u/0?cid=…` est une page web, pas un
  flux : utilisez l’« Adresse secrète au format iCal » de Google Calendar.
- Aucun événement : vérifiez que le flux contient des `VEVENT`, que le fuseau et
  les dates sont dans la plage affichée, puis appuyez sur `r`. L'écran `i`
  détaille l'état de chaque source et les avertissements de lecture.
- La molette peut ne pas être transmise par certains terminaux ou multiplexeurs ;
  les flèches et `PageUp`/`PageDown` restent disponibles.

## Développement

```bash
cargo fmt --check
cargo clippy --all-targets
cargo test
```

Le projet est organisé autour de quelques modules : `config` (fichier home et
validation), `ics` (analyse iCalendar et récurrences), `fetch` (téléchargement
en arrière-plan), `calendar`/`layout` (dates et géométrie), `theme` (couleurs),
`app` (état et entrées) et `ui` (rendu ANSI).
