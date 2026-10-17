//! Diffs computed off the event loop.
//!
//! Reading the two blobs and running the diff engine can take over a second
//! for one file, so neither happens on the thread that handles keys. The
//! session asks for the diff of the selected file and its neighbours; a few
//! worker threads answer, and finished diffs are kept in a small cache.
//! A request that has not started when the Reviewer moves on is dropped, so
//! fast paging never builds a queue.

use std::collections::{HashSet, VecDeque};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};

use super::Loaded;
use crate::diff::FileDiff;
use crate::git::Repo;

/// Computes the diff of a file: its path and the two versions (`None` where
/// the file does not exist). The session uses the real engine; tests put in a
/// slow one.
pub type DiffEngine = Arc<dyn Fn(&str, Option<&[u8]>, Option<&[u8]>) -> FileDiff + Send + Sync>;

/// Finished diffs kept for going back and forth.
const CACHE_SIZE: usize = 32;

/// Where one side of a diff comes from.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum Source {
    Absent,
    /// The file at a revision.
    Rev(String, String),
    /// A blob by id.
    Blob(String),
}

/// What a diff is cached under: the same key always means the same diff.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct Key {
    pub(super) path: String,
    pub(super) old: Source,
    pub(super) new: Source,
}

pub(super) type Computed = Result<Arc<Loaded>, String>;

struct Job {
    key: Key,
    engine: DiffEngine,
    generation: u64,
}

#[derive(Default)]
struct Queue {
    /// Most wanted first.
    jobs: VecDeque<Job>,
    /// Keys being worked on, with the generation they were asked for in.
    running: HashSet<(Key, u64)>,
    stop: bool,
}

struct Shared {
    queue: Mutex<Queue>,
    wake: Condvar,
}

pub(super) struct Diffs {
    shared: Arc<Shared>,
    receiver: Receiver<(u64, Key, Computed)>,
    cache: VecDeque<(Key, Computed)>,
    /// Bumped when the engine changes: older answers are discarded.
    generation: u64,
    engine: DiffEngine,
}

fn real_engine() -> DiffEngine {
    Arc::new(|path, old, new| crate::diff::diff_file(path, old, new, crate::diff::session_limits()))
}

fn read(repo: &Repo, source: &Source) -> Result<Option<Vec<u8>>, String> {
    match source {
        Source::Absent => Ok(None),
        Source::Rev(rev, path) => repo.file_at(rev, path).map(Some).map_err(|e| e.0),
        Source::Blob(id) => repo.blob(id).map(Some).map_err(|e| e.0),
    }
}

fn compute(repo: &Repo, job: &Job) -> Computed {
    catch_unwind(AssertUnwindSafe(|| {
        let old = read(repo, &job.key.old)?;
        let new = read(repo, &job.key.new)?;
        let diff = (job.engine)(&job.key.path, old.as_deref(), new.as_deref());
        Ok(Arc::new(Loaded::new(diff)))
    }))
    .unwrap_or_else(|_| Err("The diff engine failed on this file".to_string()))
}

/// The name of the worker threads.
const WORKER: &str = "moonhare-diff";

/// Keep the panic message of a diff or Forge worker (and the terminal restore that the
/// hooks installed before this one do) away from the TUI: the worker catches
/// its own panic and the session shows it as an error. Other threads panic as
/// before. Call it after the other hooks are set, so this one runs first.
pub fn silence_worker_panics() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if !matches!(
            std::thread::current().name(),
            Some(WORKER | super::forge_load::WORKER)
        ) {
            previous(info);
        }
    }));
}

fn work(shared: Arc<Shared>, repo: Repo, sender: Sender<(u64, Key, Computed)>) {
    loop {
        let job = {
            let mut queue = shared.queue.lock().unwrap();
            loop {
                if queue.stop {
                    return;
                }
                if let Some(job) = queue.jobs.pop_front() {
                    queue.running.insert((job.key.clone(), job.generation));
                    break job;
                }
                queue = shared.wake.wait(queue).unwrap();
            }
        };
        let result = compute(&repo, &job);
        // Send first: between the two, a key must not look idle and unanswered.
        let sent = sender.send((job.generation, job.key.clone(), result));
        shared
            .queue
            .lock()
            .unwrap()
            .running
            .remove(&(job.key, job.generation));
        if sent.is_err() {
            return;
        }
    }
}

impl Diffs {
    pub(super) fn new(repo: &Repo) -> Diffs {
        let shared = Arc::new(Shared {
            queue: Mutex::default(),
            wake: Condvar::new(),
        });
        let (sender, receiver) = channel();
        let workers = std::thread::available_parallelism()
            .map_or(2, |n| n.get())
            .clamp(2, 4);
        for _ in 0..workers {
            let (shared, repo, sender) = (shared.clone(), repo.clone(), sender.clone());
            let _ = std::thread::Builder::new()
                .name(WORKER.into())
                .spawn(move || work(shared, repo, sender));
        }
        Diffs {
            shared,
            receiver,
            cache: VecDeque::new(),
            generation: 0,
            engine: real_engine(),
        }
    }

    /// Use another engine from now on; what was computed is forgotten.
    pub(super) fn set_engine(&mut self, engine: DiffEngine) {
        self.engine = engine;
        self.generation += 1;
        self.cache.clear();
        self.shared.queue.lock().unwrap().jobs.clear();
    }

    fn store(&mut self, generation: u64, key: Key, result: Computed) -> bool {
        if generation != self.generation {
            return false;
        }
        self.cache.retain(|(k, _)| *k != key);
        self.cache.push_back((key, result));
        while self.cache.len() > CACHE_SIZE {
            self.cache.pop_front();
        }
        true
    }

    /// Take in the answers that have arrived; whether there were any.
    pub(super) fn drain(&mut self) -> bool {
        let mut any = false;
        while let Ok((generation, key, result)) = self.receiver.try_recv() {
            any |= self.store(generation, key, result);
        }
        any
    }

    /// The finished diff for `key`, if there is one.
    pub(super) fn get(&mut self, key: &Key) -> Option<Computed> {
        let at = self.cache.iter().position(|(k, _)| k == key)?;
        let entry = self.cache.remove(at)?;
        let result = entry.1.clone();
        // A failure is shown once and not kept, so the next visit tries again.
        // Most recently used goes last, so it is evicted last.
        if result.is_ok() {
            self.cache.push_back(entry);
        }
        Some(result)
    }

    /// Replace the wish list: `keys`, most wanted first. Whatever waited
    /// before and is not on the list is dropped; running work finishes and
    /// lands in the cache.
    pub(super) fn want(&mut self, keys: Vec<Key>) {
        let mut queue = self.shared.queue.lock().unwrap();
        queue.jobs = keys
            .into_iter()
            .filter(|k| {
                !queue.running.contains(&(k.clone(), self.generation))
                    && !self.cache.iter().any(|(c, _)| c == k)
            })
            .map(|key| Job {
                key,
                engine: self.engine.clone(),
                generation: self.generation,
            })
            .collect();
        drop(queue);
        self.shared.wake.notify_all();
    }

    /// Block until the diff for `key` is there. The key must have been asked
    /// for with [`want`](Self::want).
    pub(super) fn wait(&mut self, key: &Key) {
        loop {
            self.drain();
            if self.cache.iter().any(|(k, _)| k == key) {
                return;
            }
            match self.receiver.recv() {
                Ok((generation, k, result)) => {
                    self.store(generation, k, result);
                }
                Err(_) => return,
            }
        }
    }
}

impl Drop for Diffs {
    fn drop(&mut self) {
        self.shared.queue.lock().unwrap().stop = true;
        self.shared.wake.notify_all();
    }
}
