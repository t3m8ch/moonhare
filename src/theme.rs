//! Themes: every colour the screen is drawn with, by role.
//!
//! The UI never names a colour itself; it asks the current [`Theme`] for the
//! colour of a role (`fg`, `dim`, `added_bg`, a syntax highlight, …). Two
//! themes are built in: [`DARK`], the long-standing look for a dark terminal,
//! and [`LIGHT`] for a light one. Which one is drawn is a [`Choice`]: `auto`
//! follows the terminal [`Background`], told by [`detect`].
//!
//! Colours are 256-palette entries so that [`crate::color::to_truecolor`]
//! renders both themes as exact RGB.

use std::fmt;
use std::str::FromStr;

use ratatui::style::Color;

use crate::color::indexed_to_rgb;
use crate::diff::Highlight;

mod detect;

pub use detect::{background_from_colorfgbg, background_from_osc11, detect, Reply};
#[cfg(unix)]
pub use detect::{query_background, query_background_on};

/// Whether the terminal draws on a dark or a light background.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Background {
    Dark,
    Light,
}

/// Which theme the Reviewer asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Choice {
    /// Follow the terminal background.
    #[default]
    Auto,
    Dark,
    Light,
}

impl Choice {
    /// The theme drawn under this choice on a terminal with `background`
    /// (dark when it is unknown).
    pub fn theme(self, background: Option<Background>) -> &'static Theme {
        match (self, background) {
            (Choice::Light, _) | (Choice::Auto, Some(Background::Light)) => &LIGHT,
            _ => &DARK,
        }
    }
}

impl FromStr for Choice {
    type Err = String;

    fn from_str(s: &str) -> Result<Choice, String> {
        match s {
            "auto" => Ok(Choice::Auto),
            "dark" => Ok(Choice::Dark),
            "light" => Ok(Choice::Light),
            other => Err(format!(
                "Unknown theme \"{other}\": use light, dark or auto"
            )),
        }
    }
}

impl fmt::Display for Choice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Choice::Auto => "auto",
            Choice::Dark => "dark",
            Choice::Light => "light",
        })
    }
}

/// Colours of the syntax highlights of code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Syntax {
    pub keyword: Color,
    pub string: Color,
    pub comment: Color,
    pub type_: Color,
    pub error: Color,
    pub delimiter: Color,
}

/// Colours of Markdown drawn in the Summary, an Explanation and the
/// description of a Group. Links and quote bars are `dim`, text is `fg`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Markdown {
    pub heading: Color,
    /// Inline code: text and background.
    pub code: Color,
    pub code_bg: Color,
    /// The background of a code block, whose text is `fg`.
    pub block_bg: Color,
}

/// The colour of every role on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub name: &'static str,
    /// The terminal background the theme is made for, as RGB: what
    /// `Color::Reset` stands for when contrast is measured.
    pub terminal_bg: (u8, u8, u8),
    /// Ordinary text.
    pub fg: Color,
    /// Muted text: hints, line numbers, borders, headers of Rounds.
    pub dim: Color,
    /// The cursor's marks, focused borders, the mode badge.
    pub accent: Color,
    /// Text drawn on `accent` or on `comment` (badges, the range chip).
    pub on_accent: Color,
    /// Errors in place of a diff.
    pub error: Color,
    /// The status bar and the file bar above the diff.
    pub bar_bg: Color,
    /// The file name in the file bar.
    pub header_fg: Color,
    /// The row under the cursor in a list.
    pub cursor_bg: Color,
    /// Selected rows and text selected with the mouse.
    pub selection_bg: Color,
    /// The placeholder row of folded unchanged lines.
    pub fold_bg: Color,
    pub removed_bg: Color,
    /// A changed fragment inside a removed line.
    pub removed_fragment_bg: Color,
    pub added_bg: Color,
    /// A changed fragment inside an added line.
    pub added_fragment_bg: Color,
    /// Open Comments and their marks.
    pub comment: Color,
    /// A Comment being written.
    pub draft: Color,
    pub status_added: Color,
    pub status_deleted: Color,
    pub status_modified: Color,
    pub status_renamed: Color,
    /// Directories in the file tree.
    pub directory: Color,
    pub syntax: Syntax,
    pub markdown: Markdown,
}

/// The long-standing look, made for a dark terminal.
pub static DARK: Theme = Theme {
    name: "dark",
    terminal_bg: (0, 0, 0),
    fg: Color::Indexed(252),
    dim: Color::Indexed(244),
    accent: Color::Indexed(75),
    on_accent: Color::Indexed(16),
    error: Color::Red,
    bar_bg: Color::Indexed(236),
    header_fg: Color::Indexed(231),
    cursor_bg: Color::Indexed(238),
    selection_bg: Color::Indexed(24),
    fold_bg: Color::Indexed(235),
    removed_bg: Color::Indexed(52),
    removed_fragment_bg: Color::Indexed(88),
    added_bg: Color::Indexed(22),
    added_fragment_bg: Color::Indexed(28),
    comment: Color::Indexed(179),
    draft: Color::Indexed(215),
    status_added: Color::Indexed(114),
    status_deleted: Color::Indexed(174),
    status_modified: Color::Indexed(179),
    status_renamed: Color::Indexed(110),
    directory: Color::Indexed(110),
    syntax: Syntax {
        keyword: Color::Indexed(176),
        string: Color::Indexed(150),
        comment: Color::Indexed(243),
        type_: Color::Indexed(110),
        error: Color::Indexed(203),
        delimiter: Color::Indexed(247),
    },
    markdown: Markdown {
        heading: Color::Indexed(75),
        code: Color::Indexed(180),
        code_bg: Color::Indexed(236),
        block_bg: Color::Indexed(235),
    },
};

/// Made for a light terminal: dark text, pale line backgrounds.
pub static LIGHT: Theme = Theme {
    name: "light",
    terminal_bg: (255, 255, 255),
    fg: Color::Indexed(235),
    dim: Color::Indexed(241),
    accent: Color::Indexed(25),
    on_accent: Color::Indexed(231),
    error: Color::Indexed(124),
    bar_bg: Color::Indexed(253),
    header_fg: Color::Indexed(16),
    cursor_bg: Color::Indexed(254),
    selection_bg: Color::Indexed(189),
    fold_bg: Color::Indexed(255),
    removed_bg: Color::Indexed(224),
    removed_fragment_bg: Color::Indexed(217),
    added_bg: Color::Indexed(194),
    added_fragment_bg: Color::Indexed(157),
    comment: Color::Indexed(94),
    draft: Color::Indexed(88),
    status_added: Color::Indexed(22),
    status_deleted: Color::Indexed(124),
    status_modified: Color::Indexed(94),
    status_renamed: Color::Indexed(25),
    directory: Color::Indexed(25),
    syntax: Syntax {
        keyword: Color::Indexed(90),
        string: Color::Indexed(22),
        comment: Color::Indexed(241),
        type_: Color::Indexed(19),
        error: Color::Indexed(88),
        delimiter: Color::Indexed(238),
    },
    markdown: Markdown {
        heading: Color::Indexed(25),
        code: Color::Indexed(94),
        code_bg: Color::Indexed(255),
        block_bg: Color::Indexed(255),
    },
};

impl Theme {
    /// The colour of a syntax highlight.
    pub fn highlight(&self, h: Highlight) -> Color {
        match h {
            Highlight::Keyword => self.syntax.keyword,
            Highlight::String => self.syntax.string,
            Highlight::Comment => self.syntax.comment,
            Highlight::Type => self.syntax.type_,
            Highlight::Error => self.syntax.error,
            Highlight::Delimiter => self.syntax.delimiter,
            Highlight::Normal | Highlight::Text | Highlight::Ignorable => self.fg,
        }
    }

    /// The RGB value of a colour as this theme's terminal shows it.
    pub fn rgb(&self, c: Color) -> (u8, u8, u8) {
        match c {
            Color::Rgb(r, g, b) => (r, g, b),
            Color::Indexed(i) => indexed_to_rgb(i),
            Color::Black => indexed_to_rgb(0),
            Color::Red => indexed_to_rgb(1),
            Color::Green => indexed_to_rgb(2),
            Color::Yellow => indexed_to_rgb(3),
            Color::Blue => indexed_to_rgb(4),
            Color::Magenta => indexed_to_rgb(5),
            Color::Cyan => indexed_to_rgb(6),
            Color::Gray => indexed_to_rgb(7),
            Color::DarkGray => indexed_to_rgb(8),
            Color::LightRed => indexed_to_rgb(9),
            Color::LightGreen => indexed_to_rgb(10),
            Color::LightYellow => indexed_to_rgb(11),
            Color::LightBlue => indexed_to_rgb(12),
            Color::LightMagenta => indexed_to_rgb(13),
            Color::LightCyan => indexed_to_rgb(14),
            Color::White => indexed_to_rgb(15),
            Color::Reset => self.terminal_bg,
        }
    }

    /// The WCAG contrast ratio of `fg` on `bg`, from 1 to 21.
    pub fn contrast(&self, fg: Color, bg: Color) -> f64 {
        let a = luminance(self.rgb(fg));
        let b = luminance(self.rgb(bg));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }
}

/// WCAG relative luminance of an sRGB colour.
pub(crate) fn luminance((r, g, b): (u8, u8, u8)) -> f64 {
    let channel = |c: u8| {
        let c = c as f64 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
}
