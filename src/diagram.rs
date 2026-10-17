//! Drawing Mermaid text as box-drawing characters. `mmdflux` does the work;
//! nothing here uses images or graphics protocols of the terminal.
//!
//! The text comes from an LLM, so it is untrusted: it is refused when long,
//! rendered on a thread of its own that is given up on after a while, and a
//! crash of the renderer costs the picture, not the session.

use std::sync::mpsc::channel;
use std::sync::Once;
use std::time::Duration;

use mmdflux::{render_diagram, OutputFormat, RenderConfig};

/// Longest Mermaid source that is drawn, in bytes.
pub const MAX_SOURCE: usize = 20_000;

/// How long the renderer may take before it is given up on.
pub const RENDER_TIMEOUT: Duration = Duration::from_secs(5);

const RENDER_THREAD: &str = "moonhare-render";

/// The diagram as lines of text, or why it could not be drawn.
pub fn draw(mermaid: &str) -> Result<Vec<String>, String> {
    draw_with(mermaid, |text| {
        render_diagram(text, OutputFormat::Text, &RenderConfig::default())
            .map_err(|e| e.to_string())
    })
}

/// [`draw`] with another renderer.
pub fn draw_with(
    mermaid: &str,
    render: impl FnOnce(&str) -> Result<String, String> + Send + 'static,
) -> Result<Vec<String>, String> {
    if mermaid.len() > MAX_SOURCE {
        return Err(format!(
            "the source is too long to draw ({} bytes, the limit is {MAX_SOURCE})",
            mermaid.len()
        ));
    }
    silence_renderer_panics();
    let text = mermaid.to_string();
    let (sender, receiver) = channel();
    std::thread::Builder::new()
        .name(RENDER_THREAD.into())
        .spawn(move || {
            let rendered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| render(&text)));
            let _ = sender.send(rendered);
        })
        .map_err(|e| format!("cannot start the renderer: {e}"))?;
    match receiver.recv_timeout(RENDER_TIMEOUT) {
        Ok(Ok(Ok(text))) => Ok(text.lines().map(|l| sanitize(l.trim_end())).collect()),
        Ok(Ok(Err(e))) => Err(sanitize(&e)),
        Ok(Err(_)) => Err("the renderer crashed on this diagram".into()),
        Err(_) => Err("the diagram took too long to draw".into()),
    }
}

/// Keep the panic message of the renderer's thread off the terminal that the
/// TUI owns; other threads panic as before.
fn silence_renderer_panics() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if std::thread::current().name() != Some(RENDER_THREAD) {
                previous(info);
            }
        }));
    });
}

/// `text` with control characters (an escape sequence in it would reach the
/// terminal) replaced by a visible placeholder. Tabs become spaces.
pub fn sanitize(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '\t' => ' ',
            c if c.is_control() => '\u{fffd}',
            c => c,
        })
        .collect()
}
