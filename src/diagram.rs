//! Drawing Mermaid text as box-drawing characters. `mmdflux` does the work;
//! nothing here uses images or graphics protocols of the terminal.

use mmdflux::{render_diagram, OutputFormat, RenderConfig};

/// The diagram as lines of text, or why it could not be drawn.
pub fn draw(mermaid: &str) -> Result<Vec<String>, String> {
    // The text comes from an LLM and the renderer is a parser: a bug in it
    // must cost the Reviewer the picture, not the session.
    let rendered = std::panic::catch_unwind(|| {
        render_diagram(mermaid, OutputFormat::Text, &RenderConfig::default())
    });
    match rendered {
        Ok(Ok(text)) => Ok(text.lines().map(|l| l.trim_end().to_string()).collect()),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err("the renderer crashed on this diagram".into()),
    }
}
