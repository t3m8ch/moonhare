//! THROWAWAY PROTOTYPE - do not ship, do not extend, delete when the question is answered.
//!
//! Question: are the pure-Rust Mermaid-to-text libraries (`mermaid-text`,
//! `mmdflux`, `merman` + `merman-ascii`) good enough to render sequence
//! diagrams and flowcharts inside a code-review TUI (moonhare)? Concretely:
//! does the output fit a requested column budget (80/120/160), stay readable
//! (no overlapping boxes, unambiguous arrows), cope with Cyrillic and wide
//! (CJK/emoji) characters, render fast enough, and fail cleanly (Err, not
//! panic or silent garbage) on invalid input?
//!
//! Renders the same inline .mmd strings with every candidate, prints the real
//! output of each render, and a summary table at the end. No tests, no
//! persistence. Run: `cargo run --release > out.txt` (with CARGO_TARGET_DIR
//! pointed outside the repo).

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Instant;

// ---------------------------------------------------------------- inputs ---

const SEQ_EN: &str = r#"sequenceDiagram
    participant Client
    participant RetryPolicy
    participant HttpTransport
    participant Server
    participant Metrics
    Client->>RetryPolicy: start(request)
    loop until success or attempts exhausted
        Client->>HttpTransport: send(request)
        HttpTransport->>Server: POST /v1/review
        alt 2xx response
            Server-->>HttpTransport: 200 OK
            HttpTransport-->>Client: Response
            Client->>Metrics: record_success(attempt)
        else 5xx or timeout
            Server-->>HttpTransport: 503 Service Unavailable
            HttpTransport-->>Client: Err(Retryable)
            Client->>Metrics: record_failure(attempt)
            Client->>RetryPolicy: next_delay(attempt)
            RetryPolicy-->>Client: backoff 2^n * 100ms + jitter
        end
    end
    Client->>Metrics: record_give_up(total_attempts)
"#;

const SEQ_RU: &str = r#"sequenceDiagram
    participant Client as Клиент
    participant RetryPolicy as Политика повторов
    participant HttpTransport as HTTP-транспорт
    participant Server as Сервер
    participant Metrics as Метрики
    Client->>RetryPolicy: начать(запрос)
    loop пока не успех или попытки исчерпаны
        Client->>HttpTransport: отправить(запрос)
        HttpTransport->>Server: POST /v1/review
        alt ответ 2xx
            Server-->>HttpTransport: 200 Успешно
            HttpTransport-->>Client: Ответ
            Client->>Metrics: записать_успех(попытка)
        else 5xx или таймаут
            Server-->>HttpTransport: 503 Сервис недоступен
            HttpTransport-->>Client: Ошибка(можно повторить)
            Client->>Metrics: записать_сбой(попытка)
            Client->>RetryPolicy: следующая_задержка(попытка)
            RetryPolicy-->>Client: отсрочка 2^n * 100мс + дрожание
        end
    end
    Client->>Metrics: записать_отказ(всего_попыток)
"#;

// 8 nodes, 9 edges, one cycle: review -> diff -> parser -> review.
const FLOW_BODY: &str = r#"
    cli[cli] --> app[app]
    app --> config[config]
    app --> review[review]
    review --> diff[diff]
    diff --> parser[parser]
    parser --> review
    review --> render[render]
    render --> theme[theme]
    config --> theme
"#;

const FLOW_BODY_RU: &str = r#"
    cli[консоль] --> app[приложение]
    app --> config[настройки]
    app --> review[ревью]
    review --> diff[сравнение]
    diff --> parser[разбор]
    parser --> review
    review --> render[отрисовка]
    render --> theme[тема]
    config --> theme
"#;

const PATHO: &str = r#"flowchart TD
    root[Root orchestrator that fans out to every downstream stage of the review pipeline]
    root --> n1[Parse the unified diff hunk headers and normalise mixed line endings]
    root --> n2[Resolve renamed and moved files by similarity score above fifty percent]
    root --> n3[Highlight syntax for every changed line using the language grammar registry]
    root --> n4[Collect inline review comments from the forge API with pagination handling]
    root --> n5[Render Mermaid diagrams embedded in markdown fences into terminal text]
    root --> n6[Compute word level intra line diffs for modified hunks only when requested]
    root --> n7[Fold unchanged context regions longer than twelve lines into a placeholder]
    root --> n8[Track viewed file state per reviewer and persist it between TUI sessions]
    root --> n9[Resolve code owners from CODEOWNERS and annotate files with required reviewers]
    root --> n10[Fetch CI check run statuses and group failing annotations by file and line]
    root --> n11[Build the file tree sidebar with collapsed directories and change statistics]
    root --> n12[Submit the pending review with the chosen verdict and batched line comments]
"#;

const WIDE_FLOW: &str = r#"flowchart LR
    A[用户] --> B[服务器]
    B --> C[データベース]
    B --> D[😀 ok]
"#;

const WIDE_SEQ: &str = r#"sequenceDiagram
    participant U as 用户
    participant S as 服务器
    U->>S: 请求 😀
    S-->>U: 响应完成
"#;

// Invalid inputs.
const INVALID: &[(&str, &str)] = &[
    ("unknown header", "flowchrt LR\n    A --> B\n"),
    ("flowchart: unclosed bracket", "flowchart LR\n    A[Unclosed --> B\n"),
    ("flowchart: dangling arrow", "flowchart LR\n    A --> B\n    B -->\n"),
    ("flowchart: bad direction", "flowchart XX\n    A --> B\n"),
    ("sequence: unclosed alt", "sequenceDiagram\n    A->>B: hi\n    alt never closed\n        B-->>A: x\n"),
    ("sequence: bad arrow", "sequenceDiagram\n    A-->>>B: hi\n    B->>A: ok\n"),
    ("sequence: stray end", "sequenceDiagram\n    A->>B: hi\n    end\n"),
    ("plain prose", "this is not a diagram, just some prose.\nsecond line!\n"),
    // edge inputs (valid or borderline) aimed at provoking panics
    ("EDGE empty string", ""),
    ("EDGE sequence header only", "sequenceDiagram\n"),
    ("EDGE flowchart header only", "flowchart LR\n"),
    ("EDGE self-loop, empty label, <br/>", "flowchart LR\n    A[ ] --> A\n    B[two<br/>lines] --> B\n"),
    ("EDGE NUL / zero-width / combining", "flowchart LR\n    A[a\u{0}b\u{200b}c] --> B[e\u{301}]\n"),
    ("EDGE subgraph + cycle", "flowchart TD\n    subgraph S[Group]\n        A --> B --> C --> A\n    end\n    C --> D\n"),
    ("EDGE sequence 300-char label", "sequenceDiagram\n    A->>B: LONGLABEL\n"),
];

struct Case {
    name: &'static str,
    src: String,
    /// Substrings that must appear verbatim (single line) in a faithful render.
    expect: Vec<&'static str>,
}

fn flow(dir: &str, body: &str) -> String {
    format!("flowchart {dir}{body}")
}

fn cases() -> Vec<Case> {
    let flow_en = ["cli", "app", "config", "review", "diff", "parser", "render", "theme"];
    let flow_ru = ["консоль", "приложение", "настройки", "ревью", "сравнение", "разбор", "отрисовка", "тема"];
    vec![
        Case { name: "seq-en", src: SEQ_EN.into(), expect: vec!["Client", "RetryPolicy", "HttpTransport", "Server", "Metrics", "503 Service Unavailable", "backoff 2^n * 100ms + jitter", "POST /v1/review", "record_give_up(total_attempts)"] },
        Case { name: "seq-ru", src: SEQ_RU.into(), expect: vec!["Клиент", "Политика повторов", "HTTP-транспорт", "Сервер", "Метрики", "503 Сервис недоступен", "отсрочка 2^n * 100мс + дрожание", "записать_отказ(всего_попыток)"] },
        Case { name: "flow-lr", src: flow("LR", FLOW_BODY), expect: flow_en.to_vec() },
        Case { name: "flow-td", src: flow("TD", FLOW_BODY), expect: flow_en.to_vec() },
        Case { name: "flow-ru-lr", src: flow("LR", FLOW_BODY_RU), expect: flow_ru.to_vec() },
        Case { name: "patho-td", src: PATHO.into(), expect: vec![
            "Root orchestrator that fans out to every downstream stage of the review pipeline",
            "Parse the unified diff hunk headers and normalise mixed line endings",
            "Submit the pending review with the chosen verdict and batched line comments",
            "Fetch CI check run statuses and group failing annotations by file and line",
        ] },
        Case { name: "wide-flow", src: WIDE_FLOW.into(), expect: vec!["用户", "服务器", "データベース", "😀 ok"] },
        Case { name: "wide-seq", src: WIDE_SEQ.into(), expect: vec!["用户", "服务器", "请求 😀", "响应完成"] },
    ]
}

// ------------------------------------------------------------ candidates ---

type Render = fn(&str, usize, bool) -> Result<String, String>;

/// mermaid-text: width-aware API, soft budget.
fn mt(src: &str, w: usize, ascii: bool) -> Result<String, String> {
    let f = if ascii { mermaid_text::render_ascii_with_width } else { mermaid_text::render_with_width };
    f(src, Some(w)).map_err(|e| e.to_string())
}

/// mermaid-text with the hard budget (Err(TooWide) instead of an over-wide string).
fn mt_strict(src: &str, w: usize, ascii: bool) -> Result<String, String> {
    let opts = mermaid_text::RenderOptions { max_width: Some(w), max_width_strict: true, ascii, ..Default::default() };
    mermaid_text::render_with_options(src, &opts).map_err(|e| e.to_string())
}

/// mmdflux 2.6.1: there is NO width-aware API (no `render_diagram_fitted`); width is ignored.
fn mmd(src: &str, _w: usize, ascii: bool) -> Result<String, String> {
    let fmt = if ascii { mmdflux::OutputFormat::Ascii } else { mmdflux::OutputFormat::Text };
    mmdflux::render_diagram(src, fmt, &mmdflux::RenderConfig::default()).map_err(|e| e.to_string())
}

/// mmdflux + DIY fit loop: shrink layout spacing until the widest line fits.
fn mmd_shrink(src: &str, w: usize, ascii: bool) -> Result<String, String> {
    let fmt = if ascii { mmdflux::OutputFormat::Ascii } else { mmdflux::OutputFormat::Text };
    let mut last = Err("no attempt".to_string());
    for sep in [50.0, 30.0, 20.0, 10.0, 4.0, 1.0] {
        let mut cfg = mmdflux::RenderConfig::default();
        cfg.layout.node_sep = sep;
        cfg.layout.rank_sep = sep;
        last = mmdflux::render_diagram(src, fmt, &cfg).map_err(|e| e.to_string());
        if matches!(&last, Ok(t) if max_width(t) <= w) {
            break;
        }
    }
    last
}

fn mer_impl(src: &str, w: usize, ascii: bool, strict: bool, overflow: merman::ascii::OverflowPolicy) -> Result<String, String> {
    use merman::ascii::{AsciiLayoutProfile, AsciiRenderOptions, AsciiViewportPolicy};
    use merman::{AsciiRequest, OperationControl, ParseOptions, RenderOutput, RenderRequest, Renderer};
    let options = if ascii { AsciiRenderOptions::ascii() } else { AsciiRenderOptions::unicode() }
        .with_layout_profile(AsciiLayoutProfile::Auto);
    let mut renderer = Renderer::new();
    if strict {
        renderer = renderer.with_parse_options(ParseOptions::strict());
    }
    let req = AsciiRequest {
        options,
        viewport: AsciiViewportPolicy::with_max_width(w).overflow(overflow),
        ..Default::default()
    };
    match renderer.render(RenderRequest::ascii(src, OperationControl::new(), req)) {
        Ok(RenderOutput::Ascii(Some(r))) => Ok(r.text.to_string()),
        Ok(_) => Err("<no diagram detected>".into()),
        Err(e) => Err(e.to_string()),
    }
}
use merman::ascii::OverflowPolicy as Ov;
/// Overflow=Fallback (what merman's own example uses): over-wide diagrams become a key/value model dump.
fn mer(src: &str, w: usize, ascii: bool) -> Result<String, String> { mer_impl(src, w, ascii, true, Ov::Fallback) }
/// Overflow=Allow: always the real diagram (after the Auto->Compact retry), possibly over-wide.
fn mer_allow(src: &str, w: usize, ascii: bool) -> Result<String, String> { mer_impl(src, w, ascii, true, Ov::Allow) }
/// Overflow=Error: WidthOverflow error instead of an over-wide diagram.
fn mer_error(src: &str, w: usize, ascii: bool) -> Result<String, String> { mer_impl(src, w, ascii, true, Ov::Error) }
fn mer_lenient(src: &str, w: usize, ascii: bool) -> Result<String, String> { mer_impl(src, w, ascii, false, Ov::Allow) }

// (name, fn, print full text?)
const LIBS: &[(&str, Render, bool)] = &[
    ("mermaid-text", mt, true),
    ("mermaid-text-strict", mt_strict, false),
    ("mmdflux", mmd, true),
    ("mmdflux+shrink", mmd_shrink, true),
    ("merman", mer, true),
    ("merman-allow", mer_allow, true),
    ("merman-error", mer_error, false),
];

// --------------------------------------------------------------- harness ---

fn max_width(t: &str) -> usize {
    t.lines().map(mmdflux::format::display_width).max().unwrap_or(0)
}

struct Run {
    res: Result<String, String>,
    panic: Option<String>,
    ms: f64,
}

fn run(f: Render, src: &str, w: usize, ascii: bool) -> Run {
    let call = || catch_unwind(AssertUnwindSafe(|| f(src, w, ascii)));
    let t = Instant::now();
    let first = call();
    let first_ms = t.elapsed().as_secs_f64() * 1e3;
    let mut times = vec![first_ms];
    if first_ms < 300.0 && matches!(first, Ok(Ok(_))) {
        for _ in 0..4 {
            let t = Instant::now();
            let _ = call();
            times.push(t.elapsed().as_secs_f64() * 1e3);
        }
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let ms = times[times.len() / 2];
    match first {
        Ok(res) => Run { res, panic: None, ms },
        Err(p) => {
            let msg = p
                .downcast_ref::<&str>().map(|s| s.to_string())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "<non-string panic payload>".into());
            Run { res: Err(String::new()), panic: Some(msg), ms }
        }
    }
}

fn main() {
    std::panic::set_hook(Box::new(|_| {})); // panics are recorded, not printed
    // Ad-hoc probe: `proto probe <file.mmd> <width> [ascii]` prints every candidate's output.
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("probe") {
        let src = std::fs::read_to_string(&args[2]).unwrap();
        for (lib, f, _) in LIBS.iter().filter(|l| l.2) {
            let r = run(*f, &src, args[3].parse().unwrap(), args.len() > 4);
            println!("### {lib} ({:.2}ms)\n{}", r.ms, r.res.unwrap_or_else(|e| format!("[Err] {e} {:?}", r.panic)));
        }
        return;
    }
    let mut summary: Vec<String> = Vec::new();

    println!("######## VALID INPUTS ########");
    for case in cases() {
        for ascii in [false, true] {
            for w in [80usize, 120, 160] {
                for (lib, f, print) in LIBS {
                    let r = run(*f, &case.src, w, ascii);
                    let mode = if ascii { "ascii" } else { "unicode" };
                    let row = match (&r.panic, &r.res) {
                        (Some(p), _) => format!("PANIC: {}", p.lines().next().unwrap_or("")),
                        (None, Err(e)) => format!("Err: {}", e.lines().next().unwrap_or("")),
                        (None, Ok(t)) => {
                            let mw = max_width(t);
                            let intact = case.expect.iter().filter(|e| t.contains(*e)).count();
                            let non_ascii = ascii && t.chars().any(|c| matches!(c as u32, 0x2190..=0x21FF | 0x2500..=0x25FF));
                            format!(
                                "Ok maxw={mw} {} lines={} labels={}/{}{}",
                                if mw <= w { "FITS" } else { "OVER" },
                                t.lines().count(),
                                intact,
                                case.expect.len(),
                                if non_ascii { " (box/arrow glyphs in ASCII mode!)" } else { "" }
                            )
                        }
                    };
                    summary.push(format!("{:<9} {:<7} w={:<3} {:<20} {:>7.2}ms  {}", case.name, mode, w, lib, r.ms, row));
                    println!("\n### {lib} | {} | w={w} | {mode} | {:.2}ms", case.name, r.ms);
                    if *print {
                        match (&r.panic, &r.res) {
                            (Some(p), _) => println!("[PANIC] {p}"),
                            (None, Err(e)) => println!("[Err] {e}"),
                            (None, Ok(t)) => println!("{t}"),
                        }
                    }
                    println!("### end");
                }
            }
        }
    }

    println!("\n######## INVALID / EDGE INPUTS (w=80, unicode) ########");
    let long = "x".repeat(300);
    let mut invalid_libs: Vec<(&str, Render)> = LIBS.iter().filter(|l| l.2).map(|l| (l.0, l.1)).collect();
    invalid_libs.push(("merman-lenient", mer_lenient));
    for (label, src) in INVALID {
        let src = &src.replace("LONGLABEL", &long);
        for (lib, f) in &invalid_libs {
            let r = run(*f, src, 80, false);
            let verdict = match (&r.panic, &r.res) {
                (Some(p), _) => format!("PANIC: {}", p.lines().next().unwrap_or("")),
                (None, Err(e)) => format!("Err: {}", e.lines().next().unwrap_or("")),
                (None, Ok(t)) => format!("Ok ({} lines, maxw {}) <- check for garbage", t.lines().count(), max_width(t)),
            };
            println!("\n### INVALID [{label}] {lib}: {verdict}");
            if let (None, Ok(t)) = (&r.panic, &r.res) {
                println!("{t}");
            }
            println!("### end");
            summary.push(format!("INVALID {:<36} {:<18} {}", label, lib, verdict));
        }
    }

    println!("\n######## SUMMARY ########");
    for s in summary {
        println!("{s}");
    }
}
