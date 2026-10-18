//! The theme of the session: `--theme`, `:theme` and the terminal background,
//! which the terminal tells in its replies whenever they come.

use super::*;
use crate::terminal_input::{Reply, QUERY_BACKGROUND};
use crate::theme::{Background, Choice};

#[derive(Default)]
pub(super) struct ThemeSetting {
    choice: Choice,
    /// The terminal background (as the terminal last replied, or as
    /// `$COLORFGBG` names it); `None` when unknown.
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

    /// A reply of the terminal came: under `auto` the theme follows the
    /// background it tells.
    pub fn terminal_reply(&mut self, reply: &Reply) {
        match reply {
            Reply::Background(background) => self.set_terminal_background(Some(*background)),
            Reply::Other => {}
        }
    }

    /// `:theme [light|dark|auto]`: switch, or show the theme drawn. `auto`
    /// asks the terminal for its background anew.
    pub(super) fn theme_command(&mut self, arg: &str) {
        if arg.is_empty() {
            let how = match self.theme_setting.choice {
                Choice::Auto => "auto",
                _ => "set",
            };
            self.message = Some(format!("theme: {} ({how})", self.theme().name));
            return;
        }
        match self.set_theme(arg) {
            Ok(()) if self.theme_setting.choice == Choice::Auto => {
                self.terminal_out.push(QUERY_BACKGROUND.to_string());
            }
            Ok(()) => {}
            Err(e) => self.message = Some(e),
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
