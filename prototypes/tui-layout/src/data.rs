// PROTOTYPE data: everything here is hand-written fake data.
//
// Diff DSL, one line per row, first char is the marker:
//   =text            context
//   +text@N          added by commit N (1-based)
//   -text@N          removed by commit N
//   ~old¦new@N       changed by commit N (pre-existing code)
//   ~old¦new@N<B     changed by commit N; the old text was created by commit B
// Blank lines inside a block are written as a bare marker ("=" or "+@1").

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Ctx,
    Add,
    Del,
    Chg,
}

#[derive(Clone, Debug)]
pub struct Row {
    pub kind: Kind,
    pub old: String,
    pub new: String,
    pub old_no: usize,
    pub new_no: usize,
    /// 0-based commit index; usize::MAX for plain context.
    pub commit: usize,
    /// Chg only: commit that created the old text (None = existed before the Range).
    pub born: Option<usize>,
    pub old_span: Option<(usize, usize)>,
    pub new_span: Option<(usize, usize)>,
}

pub struct Commit {
    pub sha: &'static str,
    pub msg: &'static str,
    pub round: u8,
}

pub struct Group {
    pub name: &'static str,
    pub note: &'static str,
    pub mechanical: bool,
}

pub struct File {
    pub path: &'static str,
    pub group: usize,
    pub commits: Vec<usize>,
    pub rows: Vec<Row>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CState {
    Open,
    Sent,
}

pub struct Comment {
    pub file: usize,
    pub a: usize,
    pub b: usize,
    pub text: String,
    pub state: CState,
    pub commit: usize,
}

pub fn commits() -> Vec<Commit> {
    vec![
        Commit { sha: "a1f3c2e", msg: "retry: add exponential backoff", round: 1 },
        Commit { sha: "7be09d4", msg: "retry: policy and retryable errors", round: 1 },
        Commit { sha: "c40d8a1", msg: "client: wire retry into send()", round: 1 },
        Commit { sha: "29e7f60", msg: "tests: retry and backoff coverage", round: 1 },
        Commit { sha: "d8b2e17", msg: "backoff: cap delay, bound jitter", round: 2 },
        Commit { sha: "5c91a0b", msg: "client: honor Retry-After on 429", round: 2 },
    ]
}

pub fn groups() -> Vec<Group> {
    vec![
        Group { name: "Ядро retry", note: "новая логика backoff и политики повторов", mechanical: false },
        Group { name: "Интеграция в клиент", note: "вызов retry в HTTP-клиенте и конфиг", mechanical: false },
        Group { name: "Тесты", note: "unit и интеграционные тесты retry", mechanical: false },
        Group { name: "Механическое", note: "манифест, lock-файл, снапшот, changelog", mechanical: true },
    ]
}

const MOD_RS: &str = r##"
+//! Retry with exponential backoff.@1
+@1
~mod backoff;¦pub mod backoff;@2<1
+pub mod policy;@2
+@2
+pub use backoff::Backoff;@1
+pub use policy::{RetryPolicy, Retryable};@2
+@1
+/// Run `op` until it succeeds or the policy gives up.@1
+pub async fn run<T, E, F, Fut>(@1
+    policy: &RetryPolicy,@1
+    mut op: F,@1
+) -> Result<T, E>@1
+where@1
+    F: FnMut() -> Fut,@1
+    Fut: Future<Output = Result<T, E>>,@1
+    E: Retryable,@1
+{@1
+    let mut attempt = 0;@1
+    loop {@1
+        match op().await {@1
+            Ok(v) => return Ok(v),@1
+            Err(e) if !policy.should_retry(&e, attempt) => {@2
+                return Err(e);@2
+            }@2
+            Err(_) => {@1
+                sleep(policy.delay(attempt)).await;@1
+                attempt += 1;@1
+            }@1
+        }@1
+    }@1
+}@1
"##;

const BACKOFF_RS: &str = r##"
+use std::time::Duration;@1
+@1
+/// Exponential backoff with jitter.@1
+#[derive(Debug, Clone)]@1
+pub struct Backoff {@1
+    base: Duration,@1
+    max: Duration,@1
+    jitter: f64,@1
+}@1
+@1
+impl Backoff {@1
+    pub fn new(base: Duration, max: Duration) -> Self {@1
+        Self { base, max, jitter: 0.2 }@1
+    }@1
+@1
+    /// Delay before attempt number `attempt` (0-based).@1
+    pub fn delay(&self, attempt: u32) -> Duration {@1
~        let raw = self.base * 2u32.pow(attempt);¦        let exp = 2u32.saturating_pow(attempt.min(16));@5<1
+        let raw = (self.base * exp).min(self.max);@5
+        let spread = raw.as_secs_f64() * self.jitter;@1
~        raw + Duration::from_secs_f64(spread * rand())¦        (raw + Duration::from_secs_f64(spread * rand())).min(self.max)@5<1
+    }@1
+}@1
+@1
+fn rand() -> f64 {@1
+    // TODO: use a real RNG@1
+    0.5@1
+}@1
"##;

const POLICY_RS: &str = r##"
+use std::time::Duration;@2
+@2
+use super::Backoff;@2
+@2
+/// Errors that know whether a retry can help.@2
+pub trait Retryable {@2
+    fn is_retryable(&self) -> bool;@2
+}@2
+@2
+#[derive(Debug, Clone)]@2
+pub struct RetryPolicy {@2
+    pub max_attempts: u32,@2
+    backoff: Backoff,@2
+}@2
+@2
+impl Default for RetryPolicy {@2
+    fn default() -> Self {@2
+        Self {@2
~            max_attempts: 5,¦            max_attempts: 4,@5<2
+            backoff: Backoff::new(@2
+                Duration::from_millis(200),@2
+                Duration::from_secs(10),@2
+            ),@2
+        }@2
+    }@2
+}@2
+@2
+impl RetryPolicy {@2
+    pub fn should_retry<E: Retryable>(&self, e: &E, attempt: u32) -> bool {@2
+        attempt + 1 < self.max_attempts && e.is_retryable()@2
+    }@2
+@2
+    pub fn delay(&self, attempt: u32) -> Duration {@2
+        self.backoff.delay(attempt)@2
+    }@2
+}@2
"##;

const ERROR_RS: &str = r##"
=use std::fmt;
=
=#[derive(Debug)]
=pub enum Error {
=    Timeout,
=    Connect(String),
=    Status(u16),
+    RateLimited { retry_after: Option<u64> },@2
=}
=
=impl fmt::Display for Error {
=    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
=        write!(f, "{self:?}")
=    }
=}
+@2
+impl crate::retry::Retryable for Error {@2
+    fn is_retryable(&self) -> bool {@2
+        match self {@2
+            Error::Timeout | Error::Connect(_) => true,@2
+            Error::Status(s) => *s >= 500,@2
+            Error::RateLimited { .. } => true,@2
+        }@2
+    }@2
+}@2
"##;

const CLIENT_RS: &str = r##"
=use crate::config::Config;
=use crate::error::Error;
+use crate::retry::{self, RetryPolicy};@3
=
=pub struct Client {
=    http: reqwest::Client,
=    config: Config,
+    policy: RetryPolicy,@3
=}
=
=impl Client {
=    pub fn new(config: Config) -> Self {
~        Self { http: reqwest::Client::new(), config }¦        Self { http: reqwest::Client::new(), policy: config.retry.clone(), config }@3
=    }
=
=    pub async fn send(&self, req: Request) -> Result<Response, Error> {
-        self.send_once(&req).await@3
+        retry::run(&self.policy, || self.send_once(&req)).await@3
=    }
=
=    async fn send_once(&self, req: &Request) -> Result<Response, Error> {
=        let resp = self.http.execute(req.build()?).await?;
=        match resp.status().as_u16() {
=            200..=299 => Ok(Response::from(resp)),
+            429 => Err(rate_limited(&resp)),@6
=            s => Err(Error::Status(s)),
=        }
=    }
=}
+@6
+fn rate_limited(resp: &reqwest::Response) -> Error {@6
+    let secs = resp@6
+        .headers()@6
+        .get("retry-after")@6
+        .and_then(|v| v.to_str().ok())@6
+        .and_then(|v| v.parse().ok());@6
+    Error::RateLimited { retry_after: secs }@6
+}@6
"##;

const REQUEST_RS: &str = r##"
=use http::Method;
=
=pub struct Request {
=    pub method: Method,
=    pub url: String,
=    pub body: Option<Vec<u8>>,
+    pub idempotent: bool,@3
=}
=
=impl Request {
+    /// Only idempotent requests are safe to retry.@3
+    pub fn is_retry_safe(&self) -> bool {@3
+        self.idempotent || matches!(self.method, Method::GET | Method::HEAD)@3
+    }@3
+@3
=    pub fn build(&self) -> Result<reqwest::Request, Error> {
=        let url = self.url.parse()?;
=        Ok(reqwest::Request::new(self.method.clone(), url))
=    }
+@6
+    /// Upper bound for honoring a server-sent Retry-After.@6
+    pub const MAX_RETRY_AFTER: u64 = 60;@6
=}
"##;

const CONFIG_RS: &str = r##"
=use serde::Deserialize;
+use crate::retry::RetryPolicy;@3
=
=#[derive(Debug, Clone, Deserialize)]
=pub struct Config {
=    pub base_url: String,
=    pub timeout_ms: u64,
+    #[serde(default)]@3
+    pub retry: RetryPolicy,@3
=}
"##;

const RETRY_TEST_RS: &str = r##"
+mod common;@4
+@4
+use common::flaky_server;@4
+use httpkit::{Client, Error};@4
+@4
+#[tokio::test]@4
+async fn retries_on_503_then_succeeds() {@4
+    let server = flaky_server(&[503, 503, 200]).await;@4
+    let client = Client::new(server.config());@4
+    let resp = client.get("/ping").await.unwrap();@4
+    assert_eq!(resp.status(), 200);@4
+    assert_eq!(server.hits(), 3);@4
+}@4
+@4
+#[tokio::test]@4
+async fn gives_up_after_max_attempts() {@4
+    let server = flaky_server(&[500; 10]).await;@4
+    let client = Client::new(server.config());@4
+    let err = client.get("/ping").await.unwrap_err();@4
+    assert!(matches!(err, Error::Status(500)));@4
+    assert_eq!(server.hits(), 5);@4
+}@4
+@4
+#[tokio::test]@4
+async fn does_not_retry_client_errors() {@4
+    let server = flaky_server(&[404, 200]).await;@4
+    let client = Client::new(server.config());@4
+    assert!(client.get("/x").await.is_err());@4
+    assert_eq!(server.hits(), 1);@4
+}@4
"##;

const BACKOFF_TEST_RS: &str = r##"
+use httpkit::retry::Backoff;@4
+use std::time::Duration;@4
+@4
+fn ms(n: u64) -> Duration {@4
+    Duration::from_millis(n)@4
+}@4
+@4
+#[test]@4
+fn delay_grows_exponentially() {@4
+    let b = Backoff::new(ms(100), ms(10_000));@4
+    assert!(b.delay(1) > b.delay(0));@4
+    assert!(b.delay(2) > b.delay(1));@4
+}@4
+@4
+#[test]@4
+fn delay_never_exceeds_max() {@4
+    let b = Backoff::new(ms(100), ms(1_000));@4
+    assert!(b.delay(20) <= ms(1_000));@4
+}@4
"##;

const COMMON_RS: &str = r##"
+use httpkit::Config;@4
+use std::sync::atomic::{AtomicUsize, Ordering};@4
+use std::sync::Arc;@4
+@4
+pub struct Flaky {@4
+    addr: String,@4
+    hits: Arc<AtomicUsize>,@4
+}@4
+@4
+impl Flaky {@4
+    pub fn config(&self) -> Config {@4
+        Config::for_test(&self.addr)@4
+    }@4
+@4
+    pub fn hits(&self) -> usize {@4
+        self.hits.load(Ordering::SeqCst)@4
+    }@4
+}@4
+@4
+/// Serve `script` statuses in order, then 200.@4
+pub async fn flaky_server(script: &[u16]) -> Flaky {@4
+    let hits = Arc::new(AtomicUsize::new(0));@4
+    let addr = spawn_listener(script.to_vec(), hits.clone()).await;@4
+    Flaky { addr, hits }@4
+}@4
"##;

const CARGO_TOML: &str = r##"
=[package]
=name = "httpkit"
=version = "0.4.0"
=
=[dependencies]
=reqwest = { version = "0.12", features = ["json"] }
=tokio = { version = "1", features = ["time", "macros"] }
+rand = "0.8"@1
=serde = { version = "1", features = ["derive"] }
"##;

const CARGO_LOCK: &str = r##"
=[[package]]
=name = "pin-project-lite"
=version = "0.2.14"
+@1
+[[package]]@1
+name = "rand"@1
+version = "0.8.5"@1
+source = "registry+https://github.com/rust-lang/crates.io-index"@1
+checksum = "34af8d1a0e25924bc5b7c43c079c942339d8f0a8b57c39049bef581b46327404"@1
+dependencies = [@1
+ "libc",@1
+ "rand_chacha",@1
+ "rand_core",@1
+]@1
+@1
+[[package]]@1
+name = "rand_chacha"@1
+version = "0.3.1"@1
+source = "registry+https://github.com/rust-lang/crates.io-index"@1
+checksum = "e6c10a63a0fa32252be49d21e7709d4d4baf8d231c2dbce1eaa8141b9b127d88"@1
"##;

const SNAPSHOT: &str = r##"
+---@4
+source: tests/retry_test.rs@4
+expression: policy@4
+---@4
+RetryPolicy {@4
+    max_attempts: 5,@4
+    backoff: Backoff { base: 200ms, max: 10s },@4
+}@4
"##;

const CHANGELOG: &str = r##"
=# Changelog
=
=## Unreleased
+- Retry requests with exponential backoff and jitter.@6
+- Honor `Retry-After` on 429 responses.@6
=
=## 0.4.0
=- Initial public release.
"##;

fn diff_span(a: &str, b: &str) -> ((usize, usize), (usize, usize)) {
    let ac: Vec<char> = a.chars().collect();
    let bc: Vec<char> = b.chars().collect();
    let mut p = 0;
    while p < ac.len() && p < bc.len() && ac[p] == bc[p] {
        p += 1;
    }
    let mut s = 0;
    while s < ac.len() - p && s < bc.len() - p && ac[ac.len() - 1 - s] == bc[bc.len() - 1 - s] {
        s += 1;
    }
    ((p, ac.len() - s), (p, bc.len() - s))
}

fn parse(src: &str) -> Vec<Row> {
    let mut rows = Vec::new();
    let (mut o, mut n) = (0usize, 0usize);
    for line in src.lines() {
        if line.is_empty() {
            continue;
        }
        let mut chars = line.chars();
        let marker = chars.next().unwrap();
        let rest: String = chars.collect();
        let (body, tag) = match rest.rsplit_once('@') {
            Some((b, t)) if !t.is_empty() && t.chars().all(|c| c.is_ascii_digit() || c == '<') => {
                (b.to_string(), Some(t.to_string()))
            }
            _ => (rest.clone(), None),
        };
        let (commit, born) = match &tag {
            Some(t) => match t.split_once('<') {
                Some((c, b)) => (c.parse::<usize>().unwrap() - 1, Some(b.parse::<usize>().unwrap() - 1)),
                None => (t.parse::<usize>().unwrap() - 1, None),
            },
            None => (usize::MAX, None),
        };
        let row = match marker {
            '=' => {
                o += 1;
                n += 1;
                Row { kind: Kind::Ctx, old: body.clone(), new: body, old_no: o, new_no: n, commit: usize::MAX, born: None, old_span: None, new_span: None }
            }
            '+' => {
                n += 1;
                Row { kind: Kind::Add, old: String::new(), new: body, old_no: 0, new_no: n, commit, born: None, old_span: None, new_span: None }
            }
            '-' => {
                o += 1;
                Row { kind: Kind::Del, old: body, new: String::new(), old_no: o, new_no: 0, commit, born: None, old_span: None, new_span: None }
            }
            '~' => {
                let (old, new) = body.split_once('¦').expect("chg row needs ¦");
                o += 1;
                n += 1;
                let (os, ns) = diff_span(old, new);
                Row { kind: Kind::Chg, old: old.to_string(), new: new.to_string(), old_no: o, new_no: n, commit, born, old_span: Some(os), new_span: Some(ns) }
            }
            m => panic!("bad marker {m}"),
        };
        rows.push(row);
    }
    // rows whose old text was created inside the Range have no base-file numbering; mirror the new side
    for r in rows.iter_mut() {
        if r.kind == Kind::Chg && r.born.is_some() {
            r.old_no = r.new_no;
        }
    }
    rows
}

pub fn files() -> Vec<File> {
    let defs: [(&'static str, usize, &str); 14] = [
        ("src/retry/mod.rs", 0, MOD_RS),
        ("src/retry/backoff.rs", 0, BACKOFF_RS),
        ("src/retry/policy.rs", 0, POLICY_RS),
        ("src/error.rs", 0, ERROR_RS),
        ("src/client.rs", 1, CLIENT_RS),
        ("src/client/request.rs", 1, REQUEST_RS),
        ("src/config.rs", 1, CONFIG_RS),
        ("tests/retry_test.rs", 2, RETRY_TEST_RS),
        ("tests/backoff_test.rs", 2, BACKOFF_TEST_RS),
        ("tests/common/mod.rs", 2, COMMON_RS),
        ("Cargo.toml", 3, CARGO_TOML),
        ("Cargo.lock", 3, CARGO_LOCK),
        ("tests/snapshots/retry_policy.snap", 3, SNAPSHOT),
        ("CHANGELOG.md", 3, CHANGELOG),
    ];
    defs.iter()
        .map(|(path, group, src)| {
            let rows = parse(src);
            let mut commits: Vec<usize> = rows.iter().filter(|r| r.commit != usize::MAX).map(|r| r.commit).collect();
            commits.sort();
            commits.dedup();
            File { path, group: *group, commits, rows }
        })
        .collect()
}

/// (file index, commit index) atoms that start out as Reviewed.
pub fn initial_reviewed(files: &[File]) -> Vec<(usize, usize)> {
    let idx = |p: &str| files.iter().position(|f| f.path == p).unwrap();
    vec![
        (idx("Cargo.toml"), 0),
        (idx("src/retry/mod.rs"), 0),
        (idx("src/retry/mod.rs"), 1),
        (idx("src/retry/backoff.rs"), 0), // changed again in commit 5 (Round 2): only the delta is unreviewed
        (idx("src/error.rs"), 1),
        (idx("src/config.rs"), 2),
    ]
}

fn find_row(files: &[File], path: &str, needle: &str) -> (usize, usize) {
    let fi = files.iter().position(|f| f.path == path).unwrap();
    let ri = files[fi].rows.iter().position(|r| r.new.contains(needle) || r.old.contains(needle)).unwrap();
    (fi, ri)
}

pub fn initial_comments(files: &[File]) -> Vec<Comment> {
    let (f1, r1) = find_row(files, "src/client.rs", "retry::run(");
    let (f2, r2) = find_row(files, "src/retry/backoff.rs", "let exp =");
    vec![
        Comment {
            file: f1,
            a: r1,
            b: r1,
            text: "Почему повторяем и на 4xx? Нужен явный список retryable-статусов.".into(),
            state: CState::Sent,
            commit: 2,
        },
        Comment {
            file: f2,
            a: r2,
            b: r2,
            text: "Магическое число 16: вынести в константу и объяснить.".into(),
            state: CState::Open,
            commit: 4,
        },
    ]
}
