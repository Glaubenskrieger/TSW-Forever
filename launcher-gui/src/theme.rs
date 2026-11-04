//! Colour themes of the launcher window: the three faction themes (Templars, Illuminati, Dragon) and one neutral theme
//! (Neutral / Zuberi: anthracite and silver; Zuberi is a single mage, not a fourth faction).
//!
//! All colour values live in this one table. The window reads them from the Slint global `Theme`, which [`apply`] fills; the
//! Slint file contains no colour of a theme. A new theme is one more entry in [`THEMES`] plus two lines in each `.po` file
//! (name, description); the tests then check its contrast and that every field is set.
//!
//! A theme is cosmetic. It never changes the game, the player's real faction or any game setting, and it is independent of the
//! language. No faction logo or other artwork from the game files is used; the themes are colours only.

use slint::Color;

/// The default for every installation without a `theme` key: the original Templar red.
pub const DEFAULT: &str = "templars";

/// `0xRRGGBB` plus an optional alpha (0xFF = opaque).
#[derive(Clone, Copy, Debug)]
pub struct C(pub u32, pub u8);

const fn o(rgb: u32) -> C {
    C(rgb, 0xff)
}
const fn a(rgb: u32, alpha: u8) -> C {
    C(rgb, alpha)
}

impl C {
    pub fn color(self) -> Color {
        Color::from_argb_u8(self.1, (self.0 >> 16) as u8, (self.0 >> 8) as u8, self.0 as u8)
    }
    /// Relative luminance (WCAG 2.x) of the colour itself, ignoring alpha.
    #[cfg(test)]
    fn luminance(self) -> f64 {
        let ch = |v: u32| {
            let c = (v & 0xff) as f64 / 255.0;
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * ch(self.0 >> 16) + 0.7152 * ch(self.0 >> 8) + 0.0722 * ch(self.0)
    }
}

/// WCAG contrast ratio of two opaque colours (1.0 … 21.0).
#[cfg(test)]
pub fn contrast(x: C, y: C) -> f64 {
    let (a, b) = (x.luminance(), y.luminance());
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// Every colour the window uses that depends on the theme. Field names match the Slint global `Theme`.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub ink: C,
    pub card: C,
    pub card_line: C,
    pub text: C,
    pub dim: C,
    pub faint: C,
    /// Accent surfaces (button face, progress); `accent_hot` for lines, hover and small accents; `accent_deep` for the dark end of gradients.
    pub accent: C,
    pub accent_hot: C,
    pub accent_deep: C,
    pub ok: C,
    pub warn: C,
    pub bad: C,
    pub bg_start: C,
    pub bg_mid: C,
    pub bg_end: C,
    pub glow: C,
    pub hover_hi: C,
    pub hover_lo: C,
    pub hover_border: C,
    pub btn_border: C,
    pub btn_hover: C,
    pub btn_base: C,
    pub btn_primary: C,
    pub frame: C,
    pub shadow: C,
    pub focus_ring: C,
}

#[derive(Clone, Copy, Debug)]
pub struct ThemeDef {
    pub id: &'static str,
    /// Message ids (English) of the name and the description; translated through the `.po` files.
    pub name: &'static str,
    pub description: &'static str,
    /// True for the three faction themes, false for the neutral one (Zuberi is a single mage, not a faction). Kept as data for a
    /// later grouped selector; the window does not use it yet.
    #[allow(dead_code)]
    pub faction: bool,
    pub palette: Palette,
}

pub const THEMES: &[ThemeDef] = &[
    ThemeDef {
        id: "templars",
        name: "Templars",
        description: "Dark red and black. The classic look of the launcher.",
        faction: true,
        palette: Palette {
            ink: o(0x07070a),
            card: o(0x0f0b0c),
            card_line: o(0x2b1618),
            text: o(0xece4d9),
            dim: o(0xb0a699),
            faint: o(0x7a7068),
            accent: o(0x9b1c22),
            accent_hot: o(0xcf2a32),
            accent_deep: o(0x5c0d12),
            ok: o(0x5aa069),
            warn: o(0xd1a43a),
            bad: o(0xe0555b),
            bg_start: o(0x060608),
            bg_mid: o(0x0f0709),
            bg_end: o(0x1d0a0d),
            glow: a(0x6a1219, 0x55),
            hover_hi: o(0xb3222a),
            hover_lo: o(0x701018),
            hover_border: o(0xe8454c),
            btn_border: o(0x5a2a2d),
            btn_hover: o(0x2a1316),
            btn_base: o(0x140e10),
            btn_primary: o(0x3a1013),
            frame: o(0x3a1b1e),
            shadow: a(0xc8282f, 0x55),
            focus_ring: o(0xe8a2a6),
        },
    },
    ThemeDef {
        id: "illuminati",
        name: "Illuminati",
        description: "Deep blue and black with cool accents.",
        faction: true,
        palette: Palette {
            ink: o(0x06080c),
            card: o(0x0a0e14),
            card_line: o(0x182640),
            text: o(0xe3e9f2),
            dim: o(0xa7b4c8),
            faint: o(0x768399),
            accent: o(0x1f4e9e),
            accent_hot: o(0x4d8ae6),
            accent_deep: o(0x0c2a5c),
            ok: o(0x5aa069),
            warn: o(0xd1a43a),
            bad: o(0xe0555b),
            bg_start: o(0x05070a),
            bg_mid: o(0x080d16),
            bg_end: o(0x0a1a30),
            glow: a(0x143f8c, 0x55),
            hover_hi: o(0x2a63c4),
            hover_lo: o(0x0f3270),
            hover_border: o(0x6aa3f2),
            btn_border: o(0x27446f),
            btn_hover: o(0x101d33),
            btn_base: o(0x0c121c),
            btn_primary: o(0x0e2347),
            frame: o(0x1b2e4d),
            shadow: a(0x2f6fd4, 0x55),
            focus_ring: o(0xa8c8f5),
        },
    },
    ThemeDef {
        id: "dragon",
        name: "Dragon",
        description: "Dark green and black, quiet and mysterious.",
        faction: true,
        palette: Palette {
            ink: o(0x060807),
            card: o(0x0b0f0c),
            card_line: o(0x1a2b1f),
            text: o(0xe4eadf),
            dim: o(0xa6b3a2),
            faint: o(0x748271),
            accent: o(0x1f6b3a),
            accent_hot: o(0x3fae66),
            accent_deep: o(0x0d3a1e),
            ok: o(0x5aa069),
            warn: o(0xd1a43a),
            bad: o(0xe0555b),
            bg_start: o(0x050706),
            bg_mid: o(0x08100b),
            bg_end: o(0x0c2415),
            glow: a(0x1a6b3a, 0x55),
            hover_hi: o(0x217543),
            hover_lo: o(0x0f4a26),
            hover_border: o(0x5fcf86),
            btn_border: o(0x27553a),
            btn_hover: o(0x10231a),
            btn_base: o(0x0c140f),
            btn_primary: o(0x0f3a20),
            frame: o(0x1f3a28),
            shadow: a(0x2faa5a, 0x55),
            focus_ring: o(0xa6e3bc),
        },
    },
    ThemeDef {
        id: "neutral",
        name: "Neutral / Zuberi",
        description: "Anthracite and silver. Neutral, not tied to any of the three factions.",
        faction: false,
        palette: Palette {
            ink: o(0x08090a),
            card: o(0x101112),
            card_line: o(0x2a2d31),
            text: o(0xe6e8ea),
            dim: o(0xaab0b7),
            faint: o(0x80868d),
            accent: o(0x4b525a),
            accent_hot: o(0xb9c0c8),
            accent_deep: o(0x2a2e33),
            ok: o(0x5aa069),
            warn: o(0xd1a43a),
            bad: o(0xe0555b),
            bg_start: o(0x060707),
            bg_mid: o(0x0d0e10),
            bg_end: o(0x1a1c1f),
            glow: a(0x8a929b, 0x33),
            hover_hi: o(0x626a73),
            hover_lo: o(0x343a40),
            hover_border: o(0xcfd5db),
            btn_border: o(0x40454b),
            btn_hover: o(0x1a1c1f),
            btn_base: o(0x121315),
            btn_primary: o(0x2a2e33),
            frame: o(0x34383d),
            shadow: a(0xaab0b7, 0x33),
            focus_ring: o(0xdfe3e8),
        },
    },
];

/// The theme for a stored id. Unknown or absent ids give the default, so an old or hand-edited file never breaks the window.
pub fn resolve(id: Option<&str>) -> &'static ThemeDef {
    let id = id.unwrap_or(DEFAULT);
    THEMES.iter().find(|t| t.id == id).unwrap_or_else(|| THEMES.iter().find(|t| t.id == DEFAULT).unwrap())
}

/// Index of a stored id in the selector (the default for unknown ids).
pub fn index_of(id: Option<&str>) -> i32 {
    let t = resolve(id);
    THEMES.iter().position(|x| x.id == t.id).unwrap_or(0) as i32
}

/// The id stored for a selector index (the default for anything out of range).
pub fn id_at(index: i32) -> &'static str {
    usize::try_from(index).ok().and_then(|i| THEMES.get(i)).map_or(DEFAULT, |t| t.id)
}

/// Fill the Slint global with a theme; the window redraws at once.
pub fn apply(ui: &crate::MainWindow, theme: &ThemeDef) {
    use slint::ComponentHandle;
    let t = ui.global::<crate::Theme>();
    let p = &theme.palette;
    t.set_ink(p.ink.color());
    t.set_card(p.card.color());
    t.set_card_line(p.card_line.color());
    t.set_text(p.text.color());
    t.set_dim(p.dim.color());
    t.set_faint(p.faint.color());
    t.set_red(p.accent.color());
    t.set_red_hot(p.accent_hot.color());
    t.set_red_deep(p.accent_deep.color());
    t.set_ok(p.ok.color());
    t.set_warn(p.warn.color());
    t.set_bad(p.bad.color());
    t.set_bg_start(p.bg_start.color());
    t.set_bg_mid(p.bg_mid.color());
    t.set_bg_end(p.bg_end.color());
    t.set_glow(p.glow.color());
    t.set_hover_hi(p.hover_hi.color());
    t.set_hover_lo(p.hover_lo.color());
    t.set_hover_border(p.hover_border.color());
    t.set_btn_border(p.btn_border.color());
    t.set_btn_hover(p.btn_hover.color());
    t.set_btn_base(p.btn_base.color());
    t.set_btn_primary(p.btn_primary.color());
    t.set_frame(p.frame.color());
    t.set_shadow(p.shadow.color());
    t.set_focus_ring(p.focus_ring.color());
}

#[cfg(test)]
mod tests {
    use super::*;

    const LABEL: C = o(0xfbf3ea); // text on accent buttons (fixed light colour in the window)

    #[test]
    fn the_default_is_the_original_templar_red_and_unknown_ids_fall_back_to_it() {
        assert_eq!(DEFAULT, "templars");
        assert_eq!(resolve(None).id, "templars");
        assert_eq!(resolve(Some("klingon")).id, "templars");
        assert_eq!(resolve(Some("")).id, "templars");
        assert_eq!(resolve(Some("dragon")).id, "dragon");
        assert_eq!(THEMES[0].palette.accent.0, 0x9b1c22, "the Templar accent is the colour the launcher always had");
        assert_eq!(THEMES[0].palette.ink.0, 0x07070a);
    }

    #[test]
    fn three_faction_themes_and_one_neutral_theme_with_unique_ids() {
        let ids: Vec<_> = THEMES.iter().map(|t| t.id).collect();
        assert_eq!(ids, ["templars", "illuminati", "dragon", "neutral"]);
        assert_eq!(THEMES.iter().filter(|t| t.faction).count(), 3);
        assert!(!resolve(Some("neutral")).faction, "Neutral / Zuberi is not a fourth faction");
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), THEMES.len());
        assert_eq!(THEMES.iter().filter(|t| t.name.contains("Zuberi")).count(), 1);
    }

    #[test]
    fn selector_index_and_stored_id_map_both_ways() {
        for (i, t) in THEMES.iter().enumerate() {
            assert_eq!(id_at(i as i32), t.id);
            assert_eq!(index_of(Some(t.id)), i as i32);
        }
        assert_eq!((id_at(-1), id_at(99), index_of(None), index_of(Some("zzz"))), ("templars", "templars", 0, 0));
    }

    #[test]
    fn every_theme_is_readable_text_and_labels_have_enough_contrast() {
        for t in THEMES {
            let p = &t.palette;
            let check = |what: &str, fg: C, bg: C, min: f64| {
                let r = contrast(fg, bg);
                assert!(r >= min, "{}: {what} contrast {r:.2} < {min}", t.id);
            };
            for (bgname, bg) in [("ink", p.ink), ("card", p.card), ("bg_end", p.bg_end)] {
                check(&format!("text on {bgname}"), p.text, bg, 7.0);
                check(&format!("dim on {bgname}"), p.dim, bg, 4.5);
            }
            check("faint (hints, disabled) on card", p.faint, p.card, 3.0);
            for (name, c) in [("ok", p.ok), ("warn", p.warn), ("bad", p.bad)] {
                check(&format!("status {name} on card"), c, p.card, 4.5);
            }
            // label of the big play button sits on the accent -> deep gradient and on its hover colours
            for (name, bg) in [("accent", p.accent), ("accent_deep", p.accent_deep), ("hover_hi", p.hover_hi), ("hover_lo", p.hover_lo)] {
                check(&format!("button label on {name}"), LABEL, bg, 4.5);
            }
            // secondary buttons use the theme text colour on their faces
            for (name, bg) in [("btn_base", p.btn_base), ("btn_hover", p.btn_hover), ("btn_primary", p.btn_primary)] {
                check(&format!("text on {name}"), p.text, bg, 7.0);
            }
            check("accent line/outline on card (graphics)", p.accent_hot, p.card, 3.0);
            check("accent_hot (small caption) on ink", p.accent_hot, p.ink, 3.5);
            check("focus ring on ink (graphics)", p.focus_ring, p.ink, 7.0);
        }
    }

    #[test]
    fn names_and_descriptions_exist_for_every_theme() {
        for t in THEMES {
            assert!(!t.name.is_empty() && !t.description.is_empty());
        }
    }
}
