//! Concurrent writing of the files of a build, with the outcome of a sequential one.
//!
//! A built site is thousands of small files in as many folders, and the file system dominates
//! the time of a build: on a 10 000-page library most of a fresh build is spent creating and
//! renaming files. Several writers shorten that, by an amount that depends on the system and on
//! the state of the disk (see the changelog for the measurements); more than eight gain nothing
//! in the measurements made.
//!
//! Writing concurrently must not change what a build leaves behind, so the work is cut into
//! *lanes*. Files whose paths are the same, or that Windows would store as one file (see
//! [`fold_name`]), go to one lane and are written in their original order; different lanes share
//! no file. A lane stops at its first failure, as the sequential writer did, and the error
//! reported is always the one of the file that comes first in the original order, however the
//! threads happened to run.

use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::ops::Range;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};
use std::thread;

use anaquel_diagnostics::{Code, Diagnostic};

use crate::writer::{OutputFile, write_atomically};

/// The most files written at once. Eight and sixteen writers measured the same.
const MAX_WRITERS: usize = 8;

/// Fewer lanes than this are written by the calling thread: starting threads costs more than
/// the few files can save.
const CONCURRENCY_THRESHOLD: usize = 64;

/// One file to write: where it goes and what it holds.
pub(crate) struct Job<'a> {
    /// Absolute destination under the output folder.
    pub path: PathBuf,
    /// The file as the build produced it.
    pub file: &'a OutputFile,
}

/// The form of one name under which Windows would store it: lowercase, without an NTFS stream
/// (`name:stream` is a stream of `name`), and without trailing dots and spaces (`a.` and `a ` are
/// `a`).
///
/// Folding is done on every system: it can only put more files in one lane, which costs a little
/// speed and never changes the result. A name that folds to nothing keeps its lowercase form.
fn fold_name(name: &str) -> String {
    let lowered = name.to_lowercase();
    let stream_free = lowered.split(':').next().unwrap_or_default();
    let trimmed = stream_free.trim_end_matches(['.', ' ']);
    if trimmed.is_empty() {
        lowered
    } else {
        trimmed.to_owned()
    }
}

/// The comparison form of an output path: its folded names joined with `/`.
///
/// Two paths with the same form can be one file on Windows, and a path whose form is a prefix of
/// another's (up to a `/`) would have to be a folder.
fn path_form(relative: &str) -> String {
    Path::new(relative)
        .components()
        .filter_map(|component| match component {
            Component::Normal(name) => Some(fold_name(&name.to_string_lossy())),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Refuses an output set in which one path is a file and also the folder of another path.
///
/// The disk could never hold such a set, and which of the two writes failed would depend on
/// which ran first, so the build stops before it writes anything. Names are compared in their
/// folded form on every system, so `A` and `a/b` are refused on Linux too: the same site could
/// not be written on Windows or macOS.
///
/// # Errors
/// Returns a diagnostic naming the first file, in order, that has a parent written as a file.
pub(crate) fn refuse_file_folder_conflicts(files: &[OutputFile]) -> Result<(), Diagnostic> {
    let forms: Vec<String> = files.iter().map(|file| path_form(&file.path)).collect();
    let owners: HashMap<&str, &str> = forms
        .iter()
        .zip(files)
        .map(|(form, file)| (form.as_str(), file.path.as_str()))
        .collect();
    for (form, file) in forms.iter().zip(files) {
        for (slash, _) in form.match_indices('/') {
            if let Some(parent) = owners.get(&form[..slash]) {
                return Err(Diagnostic::error(
                    Code::IO,
                    format!(
                        "refusing to write `{}`: `{parent}` is also an output file, so it cannot be a folder",
                        file.path
                    ),
                ));
            }
        }
    }
    Ok(())
}

/// Groups the indices of `jobs` into lanes, in the order in which each lane first appears.
pub(crate) fn plan_lanes(jobs: &[Job<'_>]) -> Vec<Vec<usize>> {
    let mut lane_of: HashMap<String, usize> = HashMap::new();
    let mut lanes: Vec<Vec<usize>> = Vec::new();
    for (index, job) in jobs.iter().enumerate() {
        let lane = *lane_of.entry(path_form(&job.file.path)).or_insert_with(|| {
            lanes.push(Vec::new());
            lanes.len() - 1
        });
        lanes[lane].push(index);
    }
    lanes
}

/// How many threads should write `lanes` lanes on this machine.
pub(crate) fn writer_count(lanes: usize) -> usize {
    writers_for(
        lanes,
        thread::available_parallelism().map_or(1, NonZeroUsize::get),
    )
}

/// How many threads should write `lanes` lanes on a machine with `cores` cores.
fn writers_for(lanes: usize, cores: usize) -> usize {
    if lanes < CONCURRENCY_THRESHOLD {
        return 1;
    }
    cores.clamp(1, MAX_WRITERS).min(lanes)
}

/// How many blocks of lanes each writer claims on average. Blocks are large enough for a writer to
/// stay inside a few folders, and small enough for the last ones not to leave the other writers
/// idle: with four per writer, the slowest block is a quarter of one writer's share.
const BLOCKS_PER_WRITER: usize = 4;

/// Hands out runs of consecutive lanes to the writers, one run per claim.
///
/// Lanes come in the order of the files the caller hands over, and the build hands them over
/// sorted by path, so consecutive lanes are neighbours in the folder tree. A writer that holds a
/// run creates files and folders in a few folders, and the runs held at one moment are different
/// folders as long as a folder has fewer lanes than a run. Handing out one lane at a time sent all
/// the writers into the same folder at once, and the measurements (see the changelog) point at
/// the file system serialising the creation of the entries of one folder as the cause.
///
/// The order only decides the speed, never the result: any order writes the same files. A folder
/// with thousands of files directly in it keeps the writers together whatever the run, because the
/// run then lies inside that one folder.
struct BlockQueue {
    next: AtomicUsize,
    lanes: usize,
    block: usize,
}

impl BlockQueue {
    fn new(lanes: usize, workers: usize) -> Self {
        let block = lanes
            .div_ceil(workers.max(1).saturating_mul(BLOCKS_PER_WRITER))
            .max(1);
        Self {
            next: AtomicUsize::new(0),
            lanes,
            block,
        }
    }

    /// The next run of lane indices, or `None` when every lane has been handed out.
    fn claim(&self) -> Option<Range<usize>> {
        let start = self.next.fetch_add(self.block, Ordering::Relaxed);
        (start < self.lanes).then(|| start..(start + self.block).min(self.lanes))
    }
}

/// Writes every job with `workers` threads (the calling thread when `workers` is 1 or less).
///
/// Every lane runs even when another failed, so the error returned does not depend on timing.
///
/// # Errors
/// Returns the diagnostic of the failed write whose job comes first in `jobs`.
pub(crate) fn write_lanes(
    jobs: &[Job<'_>],
    lanes: &[Vec<usize>],
    workers: usize,
) -> Result<(), Diagnostic> {
    let queue = BlockQueue::new(lanes.len(), workers);
    let failures: Mutex<Vec<(usize, Diagnostic)>> = Mutex::new(Vec::new());
    let work = || {
        while let Some(block) = queue.claim() {
            for lane in &lanes[block] {
                for &index in lane {
                    let job = &jobs[index];
                    if let Err(diagnostic) = write_atomically(&job.path, &job.file.bytes) {
                        failures
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .push((index, diagnostic));
                        break;
                    }
                }
            }
        }
    };
    if workers <= 1 {
        work();
    } else {
        thread::scope(|scope| {
            for _ in 0..workers.min(lanes.len()) {
                // `work` only borrows shared state, so it is `Copy` and every thread gets one.
                scope.spawn(work);
            }
        });
    }
    let failures = failures
        .into_inner()
        .unwrap_or_else(PoisonError::into_inner);
    match failures.into_iter().min_by_key(|(index, _)| *index) {
        Some((_, diagnostic)) => Err(diagnostic),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests;
