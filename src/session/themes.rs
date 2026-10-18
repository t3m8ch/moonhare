//! The theme of the session: `--theme`, `:theme` and the terminal background.

use super::*;
use crate::theme::{Background, Choice};

#[derive(Default)]
pub(super) struct ThemeSetting {
    choice: Choice,
    /// The terminal background (as `$COLORFGBG` names it); `None` when unknown.
    background: Option<Background>,
}

impl Session {
    /// The theme the screen is drawn with.
    pub fn theme(&self) -> &'static Theme {
        self.theme_setting
            .choice
            .theme(self.theme_setting.background)
    }

    /// Draw with `name`: `light`, `dark` or `auto` (follow the terminal
    /// background).
    pub fn set_theme(&mut self, name: &str) -> Result<(), String> {
        self.set_choice(name.parse()?);
        Ok(())
    }

    /// Draw with the theme of `choice`.
    pub fn set_choice(&mut self, choice: Choice) {
        self.theme_setting.choice = choice;
    }

    /// The terminal background, which `auto` follows.
    pub fn set_terminal_background(&mut self, background: Option<Background>) {
        self.theme_setting.background = background;
    }

    /// `:theme [light|dark|auto]`: switch, or show the theme drawn.
    pub(super) fn theme_command(&mut self, arg: &str) {
        if arg.is_empty() {
            let how = match self.theme_setting.choice {
                Choice::Auto => "auto",
                _ => "set",
            };
            self.message = Some(format!("theme: {} ({how})", self.theme().name));
            return;
        }
        if let Err(e) = self.set_theme(arg) {
            self.message = Some(e);
        }
    }

    /// `  theme:light` while the Reviewer chose one; nothing under `auto`.
    pub(super) fn theme_status(&self) -> String {
        match self.theme_setting.choice {
            Choice::Auto => String::new(),
            choice => format!("  theme:{choice}"),
        }
    }
}
