//! Color themes: light + dark pairs from well-known palettes, mapped onto the
//! design tokens (design/tokens.css stays the source of truth for the
//! default "Coo" theme and every non-color token).
//!
//! Each palette lists its official colors (source URL beside it). The CSS is
//! generated from that data so the explicit `data-theme` blocks and the
//! System (prefers-color-scheme) block can never drift apart. A few roles a
//! palette doesn't define (extra surface steps, an 8th account hue) are
//! derived by mixing two of its own colors, and text/accent colors are nudged
//! toward the palette's text color only when needed to reach WCAG contrast;
//! both are marked in the data.
//!
//! "Monokai" is the original 2006 scheme (Wimer Hazenberg), which is dark
//! only; its light variant is derived from the same accents on a warm light
//! background. Monokai Pro is not included: it's a commercial theme and no
//! official public source documents its full palette (monokai.pro exposes
//! only #2d2a2e / #19181a; the Monokai/monokai-pro-vscode repo ships no
//! theme files).

use std::fmt::Write as _;

pub const DEFAULT: &str = "pigeon";

/// A color reference in palette data.
#[derive(Clone, Copy)]
enum C {
    Hex(&'static str),
    /// Derived: `a` mixed toward `b` by `t` (0..1).
    Mix(&'static str, &'static str, f32),
}

struct ModeSpec {
    bg: [C; 4],
    text: [C; 3],
    accent: C,
    success: C,
    warning: C,
    error: C,
    info: C,
    /// Account hues 1..8 (indigo, amber, rose, violet, lime, orange, sky, magenta slots).
    hues: [C; 8],
}

pub struct Family {
    pub id: &'static str,
    pub name: &'static str,
    pub source: &'static str,
    /// Display names of the dark / light variants.
    pub variants: (&'static str, &'static str),
    dark: ModeSpec,
    light: ModeSpec,
}

use C::{Hex, Mix};

/// Every family except "pigeon" (which is tokens.css itself).
pub static FAMILIES: &[Family] = &[
    // https://github.com/altercation/solarized (README "The Values" table)
    Family {
        id: "solarized",
        name: "Solarized",
        source: "https://github.com/altercation/solarized",
        variants: ("Dark", "Light"),
        dark: ModeSpec {
            bg: [Hex("#002b36"), Hex("#073642"), Mix("#073642", "#586e75", 0.18), Mix("#073642", "#586e75", 0.30)],
            text: [Hex("#93a1a1"), Hex("#839496"), Hex("#657b83")],
            accent: Hex("#2aa198"),
            success: Hex("#859900"),
            warning: Hex("#b58900"),
            error: Hex("#dc322f"),
            info: Hex("#268bd2"),
            hues: [
                Hex("#268bd2"),
                Hex("#b58900"),
                Hex("#dc322f"),
                Hex("#6c71c4"),
                Hex("#859900"),
                Hex("#cb4b16"),
                Hex("#2aa198"),
                Hex("#d33682"),
            ],
        },
        light: ModeSpec {
            bg: [Hex("#fdf6e3"), Hex("#eee8d5"), Mix("#eee8d5", "#93a1a1", 0.18), Hex("#fdf6e3")],
            text: [Hex("#586e75"), Hex("#657b83"), Hex("#839496")],
            accent: Hex("#2aa198"),
            success: Hex("#859900"),
            warning: Hex("#b58900"),
            error: Hex("#dc322f"),
            info: Hex("#268bd2"),
            hues: [
                Hex("#268bd2"),
                Hex("#b58900"),
                Hex("#dc322f"),
                Hex("#6c71c4"),
                Hex("#859900"),
                Hex("#cb4b16"),
                Hex("#2aa198"),
                Hex("#d33682"),
            ],
        },
    },
    // Coo's own: no popular editor theme reads as mint with both a light and
    // a dark variant (Everforest and Bamboo are earthy sage), so this one is
    // drawn here. Mint-tinted surfaces (hue ~160°) with a cool spearmint
    // accent; the account hues are fresh pastels in dark and their deeper
    // counterparts in light.
    Family {
        id: "mint",
        name: "Mint",
        source: "Coo's own palette (themes.rs)",
        variants: ("Night", "Day"),
        dark: ModeSpec {
            bg: [Hex("#0f1a17"), Hex("#0b1411"), Hex("#16241f"), Hex("#1e2f29")],
            text: [Hex("#e4f4ed"), Hex("#a9c4b9"), Hex("#7f9a8f")],
            accent: Hex("#4fdfd0"),
            success: Hex("#86e08f"),
            warning: Hex("#f2c46d"),
            error: Hex("#ff7f8e"),
            info: Hex("#6cc4ff"),
            hues: [
                Hex("#6cb6ff"),
                Hex("#f2c46d"),
                Hex("#ff8fa3"),
                Hex("#b4a2ff"),
                Hex("#a6e36e"),
                Hex("#ffad72"),
                Hex("#67d9e8"),
                Hex("#f09bd8"),
            ],
        },
        light: ModeSpec {
            bg: [Hex("#f5fbf8"), Hex("#e9f5ef"), Hex("#dcefe6"), Hex("#ffffff")],
            text: [Hex("#12302a"), Hex("#3b5a50"), Hex("#5b776d")],
            // Spearmint deepened until white labels on it read at AA.
            accent: Hex("#078463"),
            success: Hex("#2f8a3a"),
            warning: Hex("#a87400"),
            error: Hex("#c93a55"),
            info: Hex("#1f6fc6"),
            hues: [
                Hex("#2f6fd0"),
                Hex("#a87400"),
                Hex("#c93a55"),
                Hex("#6e57d8"),
                Hex("#3e8a1f"),
                Hex("#c25a16"),
                Hex("#0e8a9e"),
                Hex("#b0428f"),
            ],
        },
    },
    // https://github.com/scottmckendry/cyberdream.nvim (lua/cyberdream/colors.lua:
    // default + light). Neon on near-black; its magenta is the accent.
    Family {
        id: "cyberdream",
        name: "Cyberdream",
        source: "https://github.com/scottmckendry/cyberdream.nvim",
        variants: ("Dark", "Light"),
        dark: ModeSpec {
            // bg, a sunken step below it (derived), bg_alt, and an overlay
            // step between bg_alt and bg_highlight (derived).
            bg: [Hex("#16181a"), Mix("#16181a", "#000000", 0.3), Hex("#1e2124"), Mix("#1e2124", "#3c4048", 0.35)],
            text: [Hex("#ffffff"), Mix("#ffffff", "#7b8496", 0.45), Hex("#7b8496")],
            accent: Hex("#ff5ef1"),
            success: Hex("#5eff6c"),
            warning: Hex("#ffbd5e"),
            error: Hex("#ff6e5e"),
            info: Hex("#5ea1ff"),
            hues: [
                Hex("#5ea1ff"),
                Hex("#f1ff5e"),
                Hex("#ff6e5e"),
                Hex("#bd5eff"),
                Hex("#5eff6c"),
                Hex("#ffbd5e"),
                Hex("#5ef1ff"),
                Hex("#ff5ef1"),
            ],
        },
        light: ModeSpec {
            bg: [Hex("#ffffff"), Mix("#ffffff", "#eaeaea", 0.5), Hex("#eaeaea"), Hex("#ffffff")],
            text: [Hex("#16181a"), Mix("#16181a", "#7b8496", 0.5), Hex("#7b8496")],
            accent: Hex("#d100bf"),
            success: Hex("#008b0c"),
            warning: Hex("#d17c00"),
            error: Hex("#d11500"),
            info: Hex("#0057d1"),
            hues: [
                Hex("#0057d1"),
                Hex("#997b00"),
                Hex("#d11500"),
                Hex("#a018ff"),
                Hex("#008b0c"),
                Hex("#d17c00"),
                Hex("#008c99"),
                Hex("#d100bf"),
            ],
        },
    },
    // https://github.com/folke/tokyonight.nvim (lua/tokyonight/colors/{storm,night}.lua;
    // Day is computed at runtime there, values from extras/lua/tokyonight_day.lua)
    Family {
        id: "tokyonight",
        name: "Tokyo Night",
        source: "https://github.com/folke/tokyonight.nvim",
        variants: ("Night", "Day"),
        dark: ModeSpec {
            bg: [Hex("#1a1b26"), Hex("#16161e"), Hex("#24283b"), Hex("#292e42")],
            text: [Hex("#c0caf5"), Hex("#a9b1d6"), Hex("#737aa2")],
            accent: Hex("#7aa2f7"),
            success: Hex("#9ece6a"),
            warning: Hex("#e0af68"),
            error: Hex("#f7768e"),
            info: Hex("#7dcfff"),
            hues: [
                Hex("#7aa2f7"),
                Hex("#e0af68"),
                Hex("#f7768e"),
                Hex("#bb9af7"),
                Hex("#9ece6a"),
                Hex("#ff9e64"),
                Hex("#7dcfff"),
                Hex("#1abc9c"),
            ],
        },
        light: ModeSpec {
            bg: [Hex("#e1e2e7"), Hex("#d0d5e3"), Hex("#c4c8da"), Hex("#e1e2e7")],
            text: [Hex("#3760bf"), Hex("#6172b0"), Hex("#68709a")],
            accent: Hex("#2e7de9"),
            success: Hex("#587539"),
            warning: Hex("#8c6c3e"),
            error: Hex("#f52a65"),
            info: Hex("#007197"),
            hues: [
                Hex("#2e7de9"),
                Hex("#8c6c3e"),
                Hex("#f52a65"),
                Hex("#9854f1"),
                Hex("#587539"),
                Hex("#b15c00"),
                Hex("#007197"),
                Hex("#118c74"),
            ],
        },
    },
    // https://github.com/rose-pine/palette (palette.json: main, dawn)
    Family {
        id: "rosepine",
        name: "Rosé Pine",
        source: "https://github.com/rose-pine/palette",
        variants: ("Main", "Dawn"),
        dark: ModeSpec {
            bg: [Hex("#191724"), Hex("#1f1d2e"), Hex("#26233a"), Mix("#26233a", "#6e6a86", 0.18)],
            text: [Hex("#e0def4"), Hex("#908caa"), Hex("#6e6a86")],
            accent: Hex("#9ccfd8"),
            success: Hex("#9ccfd8"),
            warning: Hex("#f6c177"),
            error: Hex("#eb6f92"),
            info: Hex("#c4a7e7"),
            // Six accents; slots 6 and 8 are mixes of neighbours.
            hues: [
                Hex("#31748f"),
                Hex("#f6c177"),
                Hex("#eb6f92"),
                Hex("#c4a7e7"),
                Hex("#9ccfd8"),
                Mix("#f6c177", "#eb6f92", 0.5),
                Hex("#ebbcba"),
                Mix("#c4a7e7", "#eb6f92", 0.55),
            ],
        },
        light: ModeSpec {
            bg: [Hex("#faf4ed"), Hex("#f2e9e1"), Mix("#f2e9e1", "#9893a5", 0.15), Hex("#fffaf3")],
            text: [Hex("#464261"), Hex("#797593"), Hex("#9893a5")],
            accent: Hex("#286983"),
            success: Hex("#286983"),
            warning: Hex("#ea9d34"),
            error: Hex("#b4637a"),
            info: Hex("#907aa9"),
            hues: [
                Hex("#286983"),
                Hex("#ea9d34"),
                Hex("#b4637a"),
                Hex("#907aa9"),
                Hex("#56949f"),
                Mix("#ea9d34", "#b4637a", 0.5),
                Hex("#d7827e"),
                Mix("#907aa9", "#b4637a", 0.55),
            ],
        },
    },
    // https://draculatheme.com/spec (Dracula and Alucard Classic tables)
    Family {
        id: "dracula",
        name: "Dracula",
        source: "https://draculatheme.com/spec",
        variants: ("Dracula", "Alucard"),
        dark: ModeSpec {
            bg: [Hex("#282A36"), Hex("#21222C"), Mix("#282A36", "#44475A", 0.45), Mix("#282A36", "#44475A", 0.7)],
            text: [Hex("#F8F8F2"), Mix("#6272A4", "#F8F8F2", 0.45), Hex("#6272A4")],
            accent: Hex("#BD93F9"),
            success: Hex("#50FA7B"),
            warning: Hex("#FFB86C"),
            error: Hex("#FF5555"),
            info: Hex("#8BE9FD"),
            hues: [
                Hex("#6272A4"),
                Hex("#F1FA8C"),
                Hex("#FF5555"),
                Hex("#BD93F9"),
                Hex("#50FA7B"),
                Hex("#FFB86C"),
                Hex("#8BE9FD"),
                Hex("#FF79C6"),
            ],
        },
        light: ModeSpec {
            bg: [Hex("#FFFBEB"), Mix("#FFFBEB", "#CFCFDE", 0.3), Mix("#FFFBEB", "#CFCFDE", 0.55), Hex("#FFFBEB")],
            text: [Hex("#1F1F1F"), Hex("#6C664B"), Hex("#6C664B")],
            accent: Hex("#644AC9"),
            success: Hex("#14710A"),
            warning: Hex("#A34D14"),
            error: Hex("#CB3A2A"),
            info: Hex("#036A96"),
            hues: [
                Hex("#6C664B"),
                Hex("#846E15"),
                Hex("#CB3A2A"),
                Hex("#644AC9"),
                Hex("#14710A"),
                Hex("#A34D14"),
                Hex("#036A96"),
                Hex("#A3144D"),
            ],
        },
    },
    // Original Monokai (Wimer Hazenberg, 2006), values from the widely used
    // port https://github.com/sickill/vim-monokai (colors/monokai.vim):
    // Normal #f8f8f2 on #272822, Comment #75715e, CursorLine #3c3d37,
    // Visual #49483e, LineNr #90908a, accents #f92672 #a6e22e #66d9ef
    // #ae81ff #e6db74 #fd971f. Dark only upstream; the light mode is derived.
    Family {
        id: "monokai",
        name: "Monokai",
        source: "https://github.com/sickill/vim-monokai",
        variants: ("Dark", "Light"),
        dark: ModeSpec {
            bg: [Hex("#272822"), Mix("#272822", "#000000", 0.22), Hex("#3C3D37"), Hex("#49483E")],
            text: [Hex("#F8F8F2"), Hex("#90908A"), Hex("#75715E")],
            accent: Hex("#A6E22E"),
            success: Hex("#A6E22E"),
            warning: Hex("#FD971F"),
            error: Hex("#F92672"),
            info: Hex("#66D9EF"),
            hues: [
                Mix("#66D9EF", "#AE81FF", 0.55),
                Hex("#E6DB74"),
                Hex("#F92672"),
                Hex("#AE81FF"),
                Hex("#A6E22E"),
                Hex("#FD971F"),
                Hex("#66D9EF"),
                Mix("#F92672", "#AE81FF", 0.5),
            ],
        },
        light: ModeSpec {
            bg: [Hex("#FAFAF6"), Mix("#FAFAF6", "#75715E", 0.07), Mix("#FAFAF6", "#75715E", 0.14), Hex("#FFFFFF")],
            text: [Hex("#272822"), Mix("#49483E", "#272822", 0.2), Mix("#75715E", "#272822", 0.25)],
            accent: Mix("#A6E22E", "#272822", 0.55),
            success: Mix("#A6E22E", "#272822", 0.55),
            warning: Mix("#FD971F", "#272822", 0.4),
            error: Mix("#F92672", "#272822", 0.2),
            info: Mix("#66D9EF", "#272822", 0.55),
            hues: [
                Mix("#66D9EF", "#AE81FF", 0.55),
                Mix("#E6DB74", "#272822", 0.55),
                Mix("#F92672", "#272822", 0.15),
                Mix("#AE81FF", "#272822", 0.25),
                Mix("#A6E22E", "#272822", 0.5),
                Mix("#FD971F", "#272822", 0.35),
                Mix("#66D9EF", "#272822", 0.5),
                Mix("#F92672", "#AE81FF", 0.5),
            ],
        },
    },
];

/// Picker entries: (id, name) including the default.
pub fn choices() -> Vec<(&'static str, &'static str)> {
    let mut v = vec![(DEFAULT, "Coo")];
    v.extend(FAMILIES.iter().map(|f| (f.id, f.name)));
    v
}

pub fn is_known(id: &str) -> bool {
    id == DEFAULT || FAMILIES.iter().any(|f| f.id == id)
}

/// A saved palette id as it is today: retired palettes map to the one that
/// took their place (Catppuccin to Mint, Gruvbox to Cyberdream); unknown ids
/// fall back to the default.
pub fn current(id: &str) -> &str {
    match id {
        "catppuccin" => "mint",
        "gruvbox" => "cyberdream",
        id if is_known(id) => id,
        _ => DEFAULT,
    }
}

// ---------- color math ----------

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgb(pub f32, pub f32, pub f32);

pub fn parse(hex: &str) -> Rgb {
    let h = hex.trim_start_matches('#');
    let n = u32::from_str_radix(h, 16).unwrap_or(0);
    Rgb(((n >> 16) & 0xff) as f32, ((n >> 8) & 0xff) as f32, (n & 0xff) as f32)
}

impl Rgb {
    fn mix(self, o: Rgb, t: f32) -> Rgb {
        Rgb(self.0 + (o.0 - self.0) * t, self.1 + (o.1 - self.1) * t, self.2 + (o.2 - self.2) * t)
    }

    pub fn hex(self) -> String {
        let c = |v: f32| v.round().clamp(0.0, 255.0) as u8;
        format!("#{:02X}{:02X}{:02X}", c(self.0), c(self.1), c(self.2))
    }

    fn triple(self) -> String {
        let c = |v: f32| v.round().clamp(0.0, 255.0) as u8;
        format!("{}, {}, {}", c(self.0), c(self.1), c(self.2))
    }

    fn rgba(self, a: f32) -> String {
        format!("rgba({}, {a:.2})", self.triple())
    }

    pub fn luminance(self) -> f32 {
        let ch = |v: f32| {
            let v = v / 255.0;
            if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * ch(self.0) + 0.7152 * ch(self.1) + 0.0722 * ch(self.2)
    }

    /// CIE L*a*b* (D65), for perceptual distance between hues (tests).
    #[cfg(test)]
    pub fn lab(self) -> (f32, f32, f32) {
        let lin = |v: f32| {
            let v = v / 255.0;
            if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        let (r, g, b) = (lin(self.0), lin(self.1), lin(self.2));
        let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
        let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
        let f = |t: f32| if t > 0.008856 { t.cbrt() } else { 7.787 * t + 16.0 / 116.0 };
        let (fx, fy, fz) = (f(x), f(y), f(z));
        (116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz))
    }
}

pub fn contrast(a: Rgb, b: Rgb) -> f32 {
    let (la, lb) = (a.luminance(), b.luminance());
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

#[cfg(test)]
pub fn delta_e(a: Rgb, b: Rgb) -> f32 {
    let (l1, a1, b1) = a.lab();
    let (l2, a2, b2) = b.lab();
    ((l1 - l2).powi(2) + (a1 - a2).powi(2) + (b1 - b2).powi(2)).sqrt()
}

fn resolve(c: C) -> Rgb {
    match c {
        Hex(h) => parse(h),
        Mix(a, b, t) => parse(a).mix(parse(b), t),
    }
}

/// Nudge `c` toward `toward` until it reaches `min` contrast on `bg`.
fn ensure(c: Rgb, bg: Rgb, toward: Rgb, min: f32) -> Rgb {
    let mut out = c;
    let mut t = 0.0;
    while contrast(out, bg) < min && t < 1.0 {
        t += 0.04;
        out = c.mix(toward, t);
    }
    out
}

// Minimum contrasts on bg-0 (WCAG 2.x): body text AA, secondary text AA,
// captions ~AA-large, accents/marks as UI components.
pub const MIN_TEXT_1: f32 = 4.5;
pub const MIN_TEXT_2: f32 = 4.5;
pub const MIN_TEXT_3: f32 = 3.5;
pub const MIN_ACCENT: f32 = 3.0;
pub const MIN_HUE_TEXT: f32 = 4.5;

/// A fully resolved mode, ready to emit.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub dark: bool,
    pub bg: [Rgb; 4],
    pub text: [Rgb; 3],
    pub accent: Rgb,
    pub on_accent: Rgb,
    pub success: Rgb,
    pub warning: Rgb,
    pub error: Rgb,
    pub info: Rgb,
    pub hues: [Rgb; 8],
    pub hue_text: [Rgb; 8],
}

fn resolve_mode(m: &ModeSpec, dark: bool) -> Resolved {
    let bg = m.bg.map(resolve);
    let raw_text = m.text.map(resolve);
    // Push away from the background: toward white in dark, black in light.
    let extreme = if dark { Rgb(255.0, 255.0, 255.0) } else { Rgb(0.0, 0.0, 0.0) };
    // Text must read on both the canvas and the sidebar; take the worse one.
    let worst = if contrast(raw_text[0], bg[1]) < contrast(raw_text[0], bg[0]) { bg[1] } else { bg[0] };
    let both = |c: Rgb, min: f32| ensure(ensure(c, worst, extreme, min), bg[0], extreme, min);
    let t1 = both(raw_text[0], MIN_TEXT_1);
    let t2 = both(raw_text[1], MIN_TEXT_2);
    let t3 = ensure(raw_text[2], bg[0], extreme, MIN_TEXT_3);
    let fix = |c: C, min: f32| ensure(resolve(c), bg[0], extreme, min);
    let accent = fix(m.accent, MIN_ACCENT);
    let hues = m.hues.map(resolve);
    let hue_text = hues.map(|h| ensure(h, bg[0], extreme, MIN_HUE_TEXT));
    let on_accent = if contrast(bg[0], accent) >= contrast(Rgb(255.0, 255.0, 255.0), accent) {
        bg[0]
    } else {
        Rgb(255.0, 255.0, 255.0)
    };
    Resolved {
        dark,
        bg,
        text: [t1, t2, t3],
        accent,
        on_accent,
        success: fix(m.success, MIN_ACCENT),
        warning: fix(m.warning, MIN_ACCENT),
        error: fix(m.error, MIN_ACCENT),
        info: fix(m.info, MIN_ACCENT),
        hues,
        hue_text,
    }
}

pub fn resolved(id: &str, dark: bool) -> Option<Resolved> {
    let f = FAMILIES.iter().find(|f| f.id == id)?;
    Some(resolve_mode(if dark { &f.dark } else { &f.light }, dark))
}

/// (text, muted, link) for the sandboxed email frame in this palette/mode.
pub fn mail_colors(id: &str, dark: bool) -> (String, String, String) {
    match resolved(id, dark) {
        Some(r) => (r.text[0].hex(), r.text[1].hex(), r.info.hex()),
        None if dark => ("#EDEEF0".into(), "#A3A6AE".into(), "#7AAEFF".into()),
        None => ("#16171A".into(), "#52555C".into(), "#2459C7".into()),
    }
}

/// The palette's accent in this mode, for the email frame's quote styling.
pub fn mail_accent(id: &str, dark: bool) -> String {
    match resolved(id, dark) {
        Some(r) => r.accent.hex(),
        None if dark => "#3CCFAE".into(),
        None => "#0A7A64".into(),
    }
}

/// A palette's surfaces for Settings' theme cards, in one mode: bg-0, bg-2,
/// bg-3, text and accent (the cards mix the window's ground and sheet from
/// these the way app.css does).
/// Whether a message's own colors look different from the theme's: in dark
/// mode always, in light mode only on a tinted sheet (Solarized's cream, say).
/// On a white sheet "paper" and "adapted" mail look alike, so the reader
/// hides its colors button and shows each message its default way.
pub fn sender_colors_matter(id: &str, dark: bool) -> bool {
    dark || !near_white(&swatch(id, false)[0])
}

fn near_white(hex: &str) -> bool {
    let h = hex.trim_start_matches('#');
    let c = |i: usize| u8::from_str_radix(h.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0);
    h.len() >= 6 && c(0) >= 0xF6 && c(2) >= 0xF6 && c(4) >= 0xF6
}

pub fn swatch(id: &str, dark: bool) -> [String; 5] {
    match resolved(id, dark) {
        Some(r) => [r.bg[0].hex(), r.bg[2].hex(), r.bg[3].hex(), r.text[0].hex(), r.accent.hex()],
        // Coo's own palette (tokens.css).
        None if dark => ["#0D0E10", "#1A1C20", "#23252B", "#EDEEF0", "#3CCFAE"].map(String::from),
        None => ["#FFFFFF", "#EFEFEC", "#FFFFFF", "#16171A", "#0A7A64"].map(String::from),
    }
}

fn declarations(r: &Resolved) -> String {
    let mut s = String::new();
    let t1 = r.text[0];
    let (soft, sel, halo, sem_soft) = if r.dark { (0.16, 0.10, 0.14, 0.14) } else { (0.12, 0.08, 0.12, 0.10) };
    let w = |s: &mut String, k: &str, v: String| {
        let _ = writeln!(s, "  --{k}: {v};");
    };
    let _ = writeln!(s, "  color-scheme: {};", if r.dark { "dark" } else { "light" });
    for (i, b) in r.bg.iter().enumerate() {
        w(&mut s, &format!("bg-{i}"), b.hex());
    }
    w(&mut s, "line", t1.rgba(if r.dark { 0.08 } else { 0.09 }));
    w(&mut s, "line-strong", t1.rgba(if r.dark { 0.15 } else { 0.17 }));
    for (i, t) in r.text.iter().enumerate() {
        w(&mut s, &format!("text-{}", i + 1), t.hex());
    }
    w(&mut s, "accent", r.accent.hex());
    w(&mut s, "on-accent", r.on_accent.hex());
    w(&mut s, "accent-soft", r.accent.rgba(soft));
    w(&mut s, "selection", r.accent.rgba(sel));
    w(&mut s, "focus-ring", r.accent.hex());
    w(&mut s, "focus-halo", r.accent.rgba(halo));
    w(&mut s, "unread", r.accent.hex());
    w(&mut s, "highlight", r.warning.rgba(if r.dark { 0.28 } else { 0.32 }));
    w(&mut s, "success", r.success.hex());
    w(&mut s, "warning", r.warning.hex());
    w(&mut s, "warning-soft", r.warning.rgba(sem_soft));
    w(&mut s, "on-warning", if r.dark { r.bg[0].hex() } else { "#FFFFFF".into() });
    w(&mut s, "error", r.error.hex());
    w(&mut s, "error-soft", r.error.rgba(sem_soft));
    w(&mut s, "on-error", if r.dark { r.bg[0].hex() } else { "#FFFFFF".into() });
    w(&mut s, "info", r.info.hex());
    w(&mut s, "info-soft", r.info.rgba(sem_soft));
    w(&mut s, "tooltip-bg", t1.hex());
    w(&mut s, "tooltip-fg", r.bg[0].hex());
    if r.dark {
        w(&mut s, "scrim", "rgba(0, 0, 0, 0.45)".into());
        w(&mut s, "shadow-popover", "0 8px 24px rgba(0, 0, 0, 0.45), 0 0 0 1px rgba(255, 255, 255, 0.07)".into());
        w(&mut s, "shadow-overlay", "0 16px 40px rgba(0, 0, 0, 0.55), 0 0 0 1px rgba(255, 255, 255, 0.08)".into());
        w(&mut s, "shadow-dialog", "0 24px 64px rgba(0, 0, 0, 0.6), 0 0 0 1px rgba(255, 255, 255, 0.08)".into());
    } else {
        w(&mut s, "scrim", t1.rgba(0.18));
        w(&mut s, "shadow-popover", format!("0 8px 24px {}, 0 0 0 1px {}", t1.rgba(0.10), t1.rgba(0.07)));
        w(&mut s, "shadow-overlay", format!("0 16px 40px {}, 0 0 0 1px {}", t1.rgba(0.14), t1.rgba(0.08)));
        w(&mut s, "shadow-dialog", format!("0 24px 64px {}, 0 0 0 1px {}", t1.rgba(0.18), t1.rgba(0.08)));
    }
    let (fill, border, tent, tent_border, selected, declined, pip) =
        if r.dark { (0.18, 0.42, 0.16, 0.60, 0.34, 0.55, 0.20) } else { (0.14, 0.38, 0.12, 0.50, 1.0, 0.6, 0.14) };
    for (k, v) in [
        ("event-fill-a", fill),
        ("event-border-a", border),
        ("event-tent-a", tent),
        ("event-tent-border-a", tent_border),
        ("event-selected-a", selected),
        ("event-declined-opacity", declined),
        ("pip-bg-a", pip),
    ] {
        w(&mut s, k, format!("{v}"));
    }
    for i in 0..8 {
        w(&mut s, &format!("hue-{}", i + 1), r.hues[i].hex());
        w(&mut s, &format!("hue-{}-rgb", i + 1), r.hues[i].triple());
        w(&mut s, &format!("hue-{}-text", i + 1), r.hue_text[i].hex());
    }
    w(&mut s, "mark-body", t1.hex());
    w(&mut s, "mark-wing", r.text[1].hex());
    w(&mut s, "mark-neck", r.accent.hex());
    w(&mut s, "mark-beak", r.warning.hex());
    s
}

/// All non-default palettes as CSS. Selectors out-rank tokens.css: an
/// explicit data-theme, and the System case mirrored from its media query.
pub fn css() -> String {
    let mut out = String::from("/* Generated by themes.rs; see sources there. */\n");
    for f in FAMILIES {
        let dark = declarations(&resolve_mode(&f.dark, true));
        let light = declarations(&resolve_mode(&f.light, false));
        let p = f.id;
        let _ = write!(
            out,
            "\n/* {name}: {source} */\n\
             :root[data-palette=\"{p}\"],\n:root[data-palette=\"{p}\"][data-theme=\"dark\"] {{\n{dark}}}\n\
             :root[data-palette=\"{p}\"][data-theme=\"light\"] {{\n{light}}}\n\
             @media (prefers-color-scheme: light) {{\n\
             :root[data-palette=\"{p}\"]:not([data-theme=\"dark\"]):not([data-theme=\"light\"]) {{\n{light}}}\n}}\n",
            name = f.name,
            source = f.source,
        );
    }
    out
}

// ---------- high contrast ----------

/// The high-contrast take on a resolved mode: the sheet pure white (or
/// black) under pure black (or white) text, secondary text and every accent
/// pushed to 7:1, hairlines made solid. The sidebar keeps a trace of the
/// palette's tint, so each theme still reads as itself.
pub fn high_contrast(r: &Resolved) -> Resolved {
    let (sheet, ink) = if r.dark {
        (Rgb(0.0, 0.0, 0.0), Rgb(255.0, 255.0, 255.0))
    } else {
        (Rgb(255.0, 255.0, 255.0), Rgb(0.0, 0.0, 0.0))
    };
    let strong = |c: Rgb, min: f32| ensure(c, sheet, ink, min);
    let accent = strong(r.accent, 7.0);
    Resolved {
        dark: r.dark,
        bg: [sheet, r.bg[1].mix(sheet, 0.55), ink.mix(sheet, if r.dark { 0.84 } else { 0.9 }), sheet],
        text: [ink, strong(r.text[1], 10.0), strong(r.text[2], 7.0)],
        accent,
        on_accent: if contrast(sheet, accent) >= contrast(ink, accent) { sheet } else { ink },
        success: strong(r.success, 4.5),
        warning: strong(r.warning, 4.5),
        error: strong(r.error, 4.5),
        info: strong(r.info, 7.0),
        hues: r.hues,
        hue_text: r.hue_text.map(|h| strong(h, 7.0)),
    }
}

/// High contrast's declarations: the palette's tokens from
/// [`high_contrast`], then solid lines and a firmer focus ring.
fn hc_declarations(r: &Resolved) -> String {
    let h = high_contrast(r);
    let mut s = declarations(&h);
    let t1 = h.text[0];
    let _ = writeln!(s, "  --line: {};", t1.rgba(0.45));
    let _ = writeln!(s, "  --line-strong: {};", t1.rgba(0.75));
    let _ = writeln!(s, "  --focus-halo: {};", h.accent.rgba(0.35));
    let _ = writeln!(s, "  --selection: {};", h.accent.rgba(0.16));
    s
}

/// Coo's own palette as a [`Resolved`], read from design/tokens.css.
fn default_resolved(dark: bool) -> Resolved {
    let tokens = include_str!("../../../design/tokens.css");
    let block = if dark {
        tokens.split(":root[data-theme=\"dark\"] {").nth(1)
    } else {
        tokens.split(":root[data-theme=\"light\"] {").nth(1)
    }
    .and_then(|b| b.split('}').next())
    .unwrap_or("");
    let get = |name: &str| -> Rgb {
        // Lines carry trailing comments, so find the declaration itself.
        block
            .find(&format!("--{name}:"))
            .map(|i| block[i + name.len() + 3..].trim_start())
            .filter(|v| v.starts_with('#') && v.len() >= 7)
            .map(|v| parse(&v[..7]))
            .unwrap_or(if dark { Rgb(255.0, 255.0, 255.0) } else { Rgb(0.0, 0.0, 0.0) })
    };
    let hues: [Rgb; 8] = std::array::from_fn(|i| get(&format!("hue-{}", i + 1)));
    let hue_text: [Rgb; 8] = std::array::from_fn(|i| get(&format!("hue-{}-text", i + 1)));
    Resolved {
        dark,
        bg: [get("bg-0"), get("bg-1"), get("bg-2"), get("bg-3")],
        text: [get("text-1"), get("text-2"), get("text-3")],
        accent: get("accent"),
        on_accent: get("on-accent"),
        success: get("success"),
        warning: get("warning"),
        error: get("error"),
        info: get("info"),
        hues,
        hue_text,
    }
}

/// Every palette's high-contrast variant, on `data-contrast="high"`
/// (out-ranking the palette's own blocks), in the same three cases.
pub fn hc_css() -> String {
    let mut out = String::from("/* High contrast (themes.rs). */\n");
    let all = std::iter::once((DEFAULT, default_resolved(true), default_resolved(false)))
        .chain(FAMILIES.iter().map(|f| (f.id, resolve_mode(&f.dark, true), resolve_mode(&f.light, false))));
    for (p, d, l) in all {
        let (dark, light) = (hc_declarations(&d), hc_declarations(&l));
        let sel = format!(":root[data-palette=\"{p}\"][data-contrast=\"high\"]");
        let _ = write!(
            out,
            "{sel},\n{sel}[data-theme=\"dark\"] {{\n{dark}}}\n\
             {sel}[data-theme=\"light\"] {{\n{light}}}\n\
             @media (prefers-color-scheme: light) {{\n{sel}:not([data-theme=\"dark\"]):not([data-theme=\"light\"]) {{\n{light}}}\n}}\n"
        );
    }
    out
}

/// A palette's card swatch in high contrast (see [`swatch`]).
pub fn swatch_hc(id: &str, dark: bool) -> [String; 5] {
    let r = high_contrast(&resolved(id, dark).unwrap_or_else(|| default_resolved(dark)));
    [r.bg[0].hex(), r.bg[2].hex(), r.bg[1].hex(), r.text[0].hex(), r.accent.hex()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn high_contrast_reaches_seven_to_one_everywhere() {
        for dark in [false, true] {
            let mut all: Vec<Resolved> =
                FAMILIES.iter().map(|f| resolve_mode(if dark { &f.dark } else { &f.light }, dark)).collect();
            all.push(default_resolved(dark));
            for r in all {
                let h = high_contrast(&r);
                assert!(contrast(h.text[0], h.bg[0]) >= 20.0);
                assert!(contrast(h.text[1], h.bg[0]) >= 7.0);
                assert!(contrast(h.accent, h.bg[0]) >= 7.0);
            }
        }
        assert!(hc_css().contains("[data-palette=\"pigeon\"][data-contrast=\"high\"]"));
        // Coo's own palette is read from tokens.css, not a fallback.
        assert_eq!(default_resolved(true).accent.hex(), "#3CCFAE");
        assert_eq!(default_resolved(false).bg[1].hex(), "#F5F5F3");
    }

    #[test]
    fn sender_colors_only_matter_off_a_white_sheet() {
        assert!(!sender_colors_matter(DEFAULT, false));
        assert!(sender_colors_matter(DEFAULT, true));
        assert!(sender_colors_matter("solarized", false));
    }

    fn all_modes() -> Vec<(&'static str, Resolved)> {
        FAMILIES
            .iter()
            .flat_map(|f| [(f.id, resolve_mode(&f.dark, true)), (f.id, resolve_mode(&f.light, false))])
            .collect()
    }

    #[test]
    fn text_contrast_meets_wcag() {
        for (id, r) in all_modes() {
            let m = if r.dark { "dark" } else { "light" };
            for (i, bg) in r.bg.iter().take(2).enumerate() {
                let c = contrast(r.text[0], *bg);
                assert!(c >= 4.5, "{id}/{m}: text-1 on bg-{i} is {c:.2}");
            }
            assert!(contrast(r.text[1], r.bg[0]) >= MIN_TEXT_2 - 0.01, "{id}/{m}: text-2");
            assert!(contrast(r.text[2], r.bg[0]) >= MIN_TEXT_3 - 0.01, "{id}/{m}: text-3");
            assert!(contrast(r.accent, r.bg[0]) >= MIN_ACCENT - 0.01, "{id}/{m}: accent");
            assert!(contrast(r.on_accent, r.accent) >= 3.0, "{id}/{m}: on-accent");
            for (i, t) in r.hue_text.iter().enumerate() {
                assert!(contrast(*t, r.bg[0]) >= MIN_HUE_TEXT - 0.01, "{id}/{m}: hue-{}-text", i + 1);
            }
        }
    }

    #[test]
    fn new_palettes_meet_aa_everywhere() {
        // Mint and Cyberdream: secondary text AA on the canvas and raised
        // surfaces, and the accent's label text AA.
        for f in FAMILIES.iter().filter(|f| ["mint", "cyberdream"].contains(&f.id)) {
            for r in [resolve_mode(&f.dark, true), resolve_mode(&f.light, false)] {
                let m = if r.dark { "dark" } else { "light" };
                // Raised surfaces (hover, chips, cards) carry text too.
                for b in [0, 2] {
                    assert!(contrast(r.text[0], r.bg[b]) >= 4.5, "{}/{m}: text-1 on bg-{b}", f.id);
                    assert!(contrast(r.text[1], r.bg[b]) >= 4.5, "{}/{m}: text-2 on bg-{b}", f.id);
                }
                assert!(contrast(r.on_accent, r.accent) >= 4.5, "{}/{m}: on-accent", f.id);
            }
        }
    }

    #[test]
    fn retired_palettes_map_to_their_replacements() {
        assert_eq!(current("catppuccin"), "mint");
        assert_eq!(current("gruvbox"), "cyberdream");
        assert_eq!(current("dracula"), "dracula");
        assert_eq!(current("nope"), DEFAULT);
        assert!(is_known(current("catppuccin")) && is_known(current("gruvbox")));
    }

    #[test]
    fn account_hues_stay_distinct() {
        for (id, r) in all_modes() {
            for i in 0..8 {
                for j in i + 1..8 {
                    let d = delta_e(r.hues[i], r.hues[j]);
                    assert!(d >= 12.0, "{id}: hue-{} vs hue-{} only ΔE {d:.1}", i + 1, j + 1);
                }
            }
        }
    }

    #[test]
    fn every_block_defines_the_same_variables() {
        let css = css();
        let mut sets: Vec<std::collections::BTreeSet<String>> = Vec::new();
        for block in css.split('{').skip(1) {
            let body = block.split('}').next().unwrap_or("");
            let vars: std::collections::BTreeSet<String> = body
                .lines()
                .filter_map(|l| l.trim().strip_prefix("--").and_then(|r| r.split(':').next()).map(String::from))
                .collect();
            if !vars.is_empty() {
                sets.push(vars);
            }
        }
        assert_eq!(sets.len(), FAMILIES.len() * 3, "dark, light and system-light per family");
        for s in &sets {
            assert_eq!(s, &sets[0]);
        }
        // And it covers every color token the design defines.
        let tokens = include_str!("../../../design/tokens.css");
        let dark_block = tokens.split(":root[data-theme=\"dark\"] {").nth(1).unwrap().split('}').next().unwrap();
        for line in dark_block.lines() {
            for decl in line.split(';') {
                if let Some(name) = decl.trim().strip_prefix("--").and_then(|r| r.split(':').next()) {
                    if name == "mark-tile" {
                        continue; // app icon tile, theme-independent
                    }
                    assert!(sets[0].contains(name), "themes.css misses --{name}");
                }
            }
        }
    }

    #[test]
    fn mail_colors_follow_palette() {
        let (fg, _, _) = mail_colors("solarized", true);
        assert_ne!(fg, "#EDEEF0");
        assert_eq!(mail_colors("pigeon", true).0, "#EDEEF0");
        assert_eq!(mail_colors("nope", false).0, "#16171A");
    }

    /// `cargo test -p pigeon-bin report_adjustments -- --ignored --nocapture`
    /// lists every color nudged away from its official value for contrast.
    #[test]
    #[ignore]
    fn report_adjustments() {
        for f in FAMILIES {
            for (spec, dark) in [(&f.dark, true), (&f.light, false)] {
                let r = resolve_mode(spec, dark);
                let pairs = [
                    ("text-1", spec.text[0], r.text[0]),
                    ("text-2", spec.text[1], r.text[1]),
                    ("text-3", spec.text[2], r.text[2]),
                    ("accent", spec.accent, r.accent),
                    ("success", spec.success, r.success),
                    ("warning", spec.warning, r.warning),
                    ("error", spec.error, r.error),
                    ("info", spec.info, r.info),
                ];
                for (k, raw, got) in pairs {
                    let raw = resolve(raw);
                    if raw.hex() != got.hex() {
                        println!(
                            "{}/{}: {k} {} -> {}",
                            f.id,
                            if dark { "dark" } else { "light" },
                            raw.hex(),
                            got.hex()
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn known_ids() {
        assert!(is_known("pigeon") && is_known("dracula") && is_known("monokai") && !is_known("monokai-pro"));
        assert_eq!(choices()[0].0, DEFAULT);
    }
}
