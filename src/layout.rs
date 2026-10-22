//! Calcul de la grille : largeur exacte, nombre de semaines, dernière rangée partielle.

pub const MIN_WIDTH: u16 = 28;
pub const MIN_HEIGHT: u16 = 6;
pub const HEADER_HEIGHT: u16 = 2; // barre de titre + en-tête des jours
pub const FOOTER_HEIGHT: u16 = 2; // une ligne de message éventuel + raccourcis
pub const GUTTER_WIDTH: u16 = 4; // « S27 »
pub const CELL_GAP: u16 = 1;
pub const MAX_ROW_HEIGHT: u16 = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub width: u16,
    pub height: u16,
    pub header_height: u16,
    pub footer_height: u16,
    pub weeks: u16,
    pub row_height: u16,
    /// Hauteur de la dernière rangée si elle est partiellement visible.
    pub last_week_height: u16,
    pub grid_height: u16,
    pub gutter_width: u16,
    pub gutter_gap: u16,
    pub cell_gap: u16,
    pub cell_widths: [u16; 7],
}

impl Layout {
    /// Renvoie `None` si le terminal est trop petit.
    ///
    /// La grille occupe toute la largeur et toute la hauteur restante. La
    /// dernière semaine peut être partiellement visible (par exemple 4 lignes
    /// sur 8) : le pied de page reste ainsi compact et la grille remplit
    /// l'écran, même avec `row_height = 8`.
    pub fn compute(
        width: u16,
        height: u16,
        requested_row_height: u16,
        show_week_numbers: bool,
    ) -> Option<Self> {
        if width < MIN_WIDTH || height < MIN_HEIGHT {
            return None;
        }

        let gutter_width = if show_week_numbers { GUTTER_WIDTH } else { 0 };
        let gutter_gap = u16::from(gutter_width > 0);
        let fixed = gutter_width + gutter_gap + CELL_GAP * 6;
        let content = width.checked_sub(fixed)?;
        if content < 7 {
            return None;
        }

        let per_cell = content / 7;
        let remainder = content % 7;
        let mut cell_widths = [per_cell; 7];
        for (i, slot) in cell_widths.iter_mut().enumerate() {
            if (i as u16) < remainder {
                *slot += 1;
            }
        }
        debug_assert_eq!(cell_widths.iter().sum::<u16>() + fixed, width);

        let header_height = HEADER_HEIGHT;
        let footer_height = FOOTER_HEIGHT;
        let grid_height = height
            .checked_sub(header_height + footer_height)
            .filter(|height| *height > 0)?;

        let requested = requested_row_height
            .clamp(1, MAX_ROW_HEIGHT)
            .min(grid_height)
            .max(1);
        let full_weeks = (grid_height / requested).max(1);
        let leftover = grid_height % requested;
        let (weeks, last_week_height) = if leftover == 0 {
            (full_weeks, requested)
        } else {
            (full_weeks + 1, leftover)
        };
        debug_assert!(weeks >= 1);
        debug_assert!(last_week_height >= 1 && last_week_height <= requested);
        debug_assert_eq!(header_height + grid_height + footer_height, height);

        Some(Self {
            width,
            height,
            header_height,
            footer_height,
            weeks,
            row_height: requested,
            last_week_height,
            grid_height,
            gutter_width,
            gutter_gap,
            cell_gap: CELL_GAP,
            cell_widths,
        })
    }

    /// Hauteur effective d'une rangée (la dernière peut être partielle).
    pub fn week_height(&self, week: u16) -> u16 {
        if week + 1 >= self.weeks {
            self.last_week_height
        } else {
            self.row_height
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_width_exactly() {
        for width in MIN_WIDTH..=200 {
            let layout = Layout::compute(width, 40, 2, true).expect("layout");
            let sum: u16 = layout.cell_widths.iter().sum();
            assert_eq!(
                sum + layout.gutter_width + layout.gutter_gap + layout.cell_gap * 6,
                width
            );
        }
    }

    #[test]
    fn fills_height_exactly() {
        for height in MIN_HEIGHT..=120 {
            for row in 1..=MAX_ROW_HEIGHT {
                let layout = Layout::compute(100, height, row, true).expect("layout");
                assert_eq!(
                    layout.header_height + layout.grid_height + layout.footer_height,
                    height
                );
                assert_eq!(layout.footer_height, FOOTER_HEIGHT);
                assert!(layout.weeks >= 1);
                assert!(layout.row_height <= row);
                assert!((1..=layout.row_height).contains(&layout.last_week_height));
                assert_eq!(
                    (u32::from(layout.weeks) - 1) * u32::from(layout.row_height)
                        + u32::from(layout.last_week_height),
                    u32::from(layout.grid_height)
                );
            }
        }
    }

    #[test]
    fn rejects_tiny_terminals() {
        assert!(Layout::compute(27, 40, 2, true).is_none());
        assert!(Layout::compute(80, 5, 2, true).is_none());
        assert!(Layout::compute(80, 6, 2, true).is_some());
    }

    #[test]
    fn gutter_can_be_hidden() {
        let layout = Layout::compute(50, 20, 2, false).unwrap();
        assert_eq!(layout.gutter_width, 0);
        assert_eq!(layout.gutter_gap, 0);
        assert_eq!(
            layout.cell_widths.iter().sum::<u16>() + layout.cell_gap * 6,
            50
        );
    }

    #[test]
    fn row_height_adapts_when_short() {
        let layout = Layout::compute(80, 6, 8, true).unwrap();
        assert_eq!(layout.grid_height, 2);
        assert_eq!(layout.row_height, 2);
        assert_eq!(layout.weeks, 1);
        assert_eq!(layout.last_week_height, 2);
        assert_eq!(
            layout.header_height + layout.grid_height + layout.footer_height,
            6
        );
    }

    #[test]
    fn tall_rows_still_fill_the_screen() {
        let layout = Layout::compute(120, 40, 8, true).unwrap();
        assert_eq!(layout.grid_height, 36);
        assert_eq!(layout.row_height, 8);
        assert_eq!(layout.weeks, 5);
        assert_eq!(layout.last_week_height, 4);
        assert_eq!(layout.footer_height, 2);
        assert_eq!(layout.week_height(0), 8);
        assert_eq!(layout.week_height(4), 4);
        assert_eq!(layout.week_height(5), 4); // dernier index valable
    }

    #[test]
    fn small_config_row_adapts_to_grid() {
        let layout = Layout::compute(80, 12, 8, false).unwrap();
        assert_eq!(layout.grid_height, 8);
        assert_eq!(layout.row_height, 8);
        assert_eq!(layout.weeks, 1);
        assert_eq!(layout.last_week_height, 8);
        assert_eq!(layout.footer_height, 2);
    }
}
