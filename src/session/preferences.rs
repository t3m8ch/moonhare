//! The Reviewer's preferences kept between runs (ADR 0008): the theme
//! `:theme` chose and the layout `s` or `:layout` chose.

use super::*;
use crate::preferences::Preferences;

const THEME: &str = "theme";
const LAYOUT: &str = "layout";

impl Session {
    /// Keep the preferences in `preferences` and apply the ones saved there.
    /// Without this call nothing is saved anywhere.
    pub fn use_preferences(&mut self, preferences: Preferences) {
        if let Some(choice) = preferences.get(THEME).and_then(|t| t.parse().ok()) {
            self.set_choice(choice);
        }
        if let Some(layout) = preferences.get(LAYOUT).and_then(parse_layout) {
            self.layout_override = layout;
        }
        self.preferences = preferences;
    }

    /// Save `key = value`; a failure is a message, the choice holds anyway.
    pub(super) fn save_preference(&mut self, key: &str, value: &str) {
        if let Err(e) = self.preferences.set(key, value) {
            self.message = Some(e);
        }
    }

    /// Save the theme the Reviewer chose.
    pub(super) fn save_theme(&mut self, choice: crate::theme::Choice) {
        self.save_preference(THEME, &choice.to_string());
    }

    /// Lay the diff out as `layout` (`None`: by width) from now on and in
    /// later runs.
    pub(super) fn choose_layout(&mut self, layout: Option<DiffLayout>) {
        self.layout_override = layout;
        self.save_preference(LAYOUT, layout_name(layout));
    }

    /// `s`: the other layout than the one drawn, as a manual choice.
    pub(super) fn toggle_layout(&mut self) {
        self.choose_layout(Some(match self.layout() {
            DiffLayout::SideBySide => DiffLayout::Inline,
            DiffLayout::Inline => DiffLayout::SideBySide,
        }));
    }

    /// `:layout [auto|inline|side-by-side]`: choose, or show the layout drawn.
    pub(super) fn layout_command(&mut self, arg: &str) {
        if arg.is_empty() {
            let how = if self.layout_override.is_some() {
                "manual"
            } else {
                "auto"
            };
            let drawn = layout_name(Some(self.layout()));
            self.message = Some(format!("layout: {drawn} ({how})"));
            return;
        }
        match parse_layout(arg) {
            Some(layout) => self.choose_layout(layout),
            None => {
                self.message = Some(format!(
                    "Unknown layout \"{arg}\": use auto, inline or side-by-side"
                ))
            }
        }
    }
}

/// `Some(None)` for `auto`; `None` for no layout name.
fn parse_layout(name: &str) -> Option<Option<DiffLayout>> {
    match name {
        "auto" => Some(None),
        "inline" => Some(Some(DiffLayout::Inline)),
        "side-by-side" => Some(Some(DiffLayout::SideBySide)),
        _ => None,
    }
}

fn layout_name(layout: Option<DiffLayout>) -> &'static str {
    match layout {
        None => "auto",
        Some(DiffLayout::Inline) => "inline",
        Some(DiffLayout::SideBySide) => "side-by-side",
    }
}
