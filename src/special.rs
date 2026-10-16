//! Labels for files that are not an ordinary modification: renamed and binary
//! files, and diffs that fell back to line-based.

use crate::diff::{FallbackReason, FileDiff};
use crate::git::ChangedFile;

/// The header of the diff pane for a file.
pub fn header(file: &ChangedFile, diff: &FileDiff) -> String {
    let mut header = match &file.old_path {
        Some(old) => format!(" {old} → {} (renamed) ", file.path),
        None => format!(" {} ", file.path),
    };
    if let Some(lang) = &diff.language {
        header.push_str(&format!("[{lang}] "));
    }
    if let Some(reason) = &diff.fallback {
        header.push_str(&format!("({}) ", fallback_note(reason)));
    }
    header
}

/// The note shown when the diff is line-based instead of structural, naming
/// the reason so a rough diff is not taken for a structural one.
pub fn fallback_note(reason: &FallbackReason) -> String {
    let why = match reason {
        FallbackReason::UnknownLanguage => "unknown language".to_string(),
        FallbackReason::ByteLimit { bytes } => format!("size limit exceeded, {bytes} bytes"),
        FallbackReason::GraphLimit => "diff too complex, graph limit exceeded".to_string(),
        FallbackReason::ParseErrors { language, errors } => {
            format!("{errors} parse errors in {language}")
        }
    };
    format!("line-based diff: {why}")
}
