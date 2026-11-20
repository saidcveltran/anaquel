use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::*;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("anaquel-lanes-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn file(path: &str, text: &str) -> OutputFile {
    OutputFile {
        path: path.to_owned(),
        bytes: text.as_bytes().to_vec(),
    }
}

fn jobs<'a>(dist: &Path, files: &'a [OutputFile]) -> Vec<Job<'a>> {
    files
        .iter()
        .map(|file| Job {
            path: dist.join(&file.path),
            file,
        })
        .collect()
}

/// Every file below `dir`, by path relative to it, with its bytes.
fn snapshot(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, found: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, found);
            } else {
                let name = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                found.insert(name, fs::read(&path).unwrap());
            }
        }
    }
    let mut found = BTreeMap::new();
    walk(dir, dir, &mut found);
    found
}

/// A set of `count` distinct files, one folder each, like the pages of a site.
fn many_files(count: usize) -> Vec<OutputFile> {
    (0..count)
        .map(|n| {
            file(
                &format!("page-{n:04}/index.html"),
                &format!("page number {n}"),
            )
        })
        .collect()
}

#[test]
fn equal_and_case_folded_paths_share_a_lane_in_their_original_order() {
    let files = [
        file("a.txt", "1"),
        file("B/c.txt", "2"),
        file("A.TXT", "3"),
        file("a.txt", "4"),
    ];
    let dist = Path::new("dist");
    assert_eq!(
        plan_lanes(&jobs(dist, &files)),
        vec![vec![0, 2, 3], vec![1]]
    );
}

#[test]
fn different_paths_never_share_a_lane() {
    let files = [
        file("Dir/x.txt", "1"),
        file("dir/y.txt", "2"),
        file("dir/x.txt/z", "3"),
    ];
    let dist = Path::new("dist");
    assert_eq!(
        plan_lanes(&jobs(dist, &files)),
        vec![vec![0], vec![1], vec![2]]
    );
}

#[test]
fn names_that_windows_stores_as_one_file_share_a_lane() {
    let files = [
        file("a", "1"),
        file("x/y.html", "2"),
        file("a.", "3"),
        file("x./y.html", "4"),
        file("a ", "5"),
        file("d/e", "6"),
        file("d/E:stream", "7"),
        file("b", "8"),
    ];
    let dist = Path::new("dist");
    assert_eq!(
        plan_lanes(&jobs(dist, &files)),
        vec![vec![0, 2, 4], vec![1, 3], vec![5, 6], vec![7]]
    );
}

#[test]
fn a_name_made_only_of_dots_keeps_its_own_form() {
    assert_eq!(fold_name("..."), "...");
    assert_eq!(fold_name("Page.HTML"), "page.html");
    assert_eq!(fold_name("note. "), "note");
    assert_eq!(fold_name("note:stream:more"), "note");
    assert_eq!(fold_name(":x"), ":x");
}

#[test]
fn the_writer_count_follows_the_cores_up_to_eight_and_the_threshold() {
    // (lanes, cores, writers)
    for (lanes, cores, expected) in [
        (0, 16, 1),
        (1, 16, 1),
        (CONCURRENCY_THRESHOLD - 1, 16, 1),
        (CONCURRENCY_THRESHOLD, 16, MAX_WRITERS),
        (CONCURRENCY_THRESHOLD, 4, 4),
        (CONCURRENCY_THRESHOLD, 1, 1),
        (CONCURRENCY_THRESHOLD, 0, 1),
        (1_000_000, 64, MAX_WRITERS),
    ] {
        assert_eq!(
            writers_for(lanes, cores),
            expected,
            "{lanes} lanes, {cores} cores"
        );
    }
    assert!((1..=MAX_WRITERS).contains(&writer_count(1_000)));
}

#[test]
fn many_threads_write_every_file_whole_and_leave_no_temporary_name() {
    let dist = scratch("whole");
    let files = many_files(600);
    let jobs = jobs(&dist, &files);
    write_lanes(&jobs, &plan_lanes(&jobs), 8).unwrap();
    let written = snapshot(&dist);
    assert_eq!(written.len(), 600);
    for output in &files {
        assert_eq!(written[&output.path], output.bytes, "{}", output.path);
    }
    assert!(written.keys().all(|name| !name.ends_with(".anaquel-tmp")));
    fs::remove_dir_all(&dist).unwrap();
}

#[test]
fn eight_threads_leave_what_one_thread_leaves() {
    let mut files = many_files(300);
    // A path written three times with different contents: the last one must win.
    files.push(file("same/index.html", "first"));
    files.push(file("same/index.html", "second"));
    files.push(file("same/index.html", "third"));
    let one = scratch("one-thread");
    let eight = scratch("eight-threads");
    let one_jobs = jobs(&one, &files);
    let eight_jobs = jobs(&eight, &files);
    write_lanes(&one_jobs, &plan_lanes(&one_jobs), 1).unwrap();
    write_lanes(&eight_jobs, &plan_lanes(&eight_jobs), 8).unwrap();
    assert_eq!(snapshot(&one), snapshot(&eight));
    assert_eq!(
        fs::read_to_string(eight.join("same/index.html")).unwrap(),
        "third"
    );
    fs::remove_dir_all(&one).unwrap();
    fs::remove_dir_all(&eight).unwrap();
}

#[test]
fn a_path_written_many_times_ends_with_its_last_content_whatever_the_timing() {
    for attempt in 0..10 {
        let dist = scratch("repeated");
        let mut files = many_files(60);
        for version in 0..40 {
            files.insert(version * 2, file("hot.txt", &format!("version {version}")));
        }
        let jobs = jobs(&dist, &files);
        write_lanes(&jobs, &plan_lanes(&jobs), 8).unwrap();
        assert_eq!(
            fs::read_to_string(dist.join("hot.txt")).unwrap(),
            "version 39",
            "attempt {attempt}"
        );
        fs::remove_dir_all(&dist).unwrap();
    }
}

/// Makes a folder where `name` should be a file, so writing that file fails on every system.
fn block(dist: &Path, name: &str) {
    fs::create_dir_all(dist.join(name)).unwrap();
}

#[test]
fn the_error_is_the_one_of_the_first_failing_file_however_the_threads_ran() {
    for workers in [1, 2, 8] {
        for attempt in 0..8 {
            let dist = scratch("first-error");
            let files: Vec<OutputFile> = (0..60)
                .map(|n| file(&format!("f{n:03}.txt"), "x"))
                .collect();
            for failing in [45, 12, 30] {
                block(&dist, &format!("f{failing:03}.txt"));
            }
            let jobs = jobs(&dist, &files);
            let error = write_lanes(&jobs, &plan_lanes(&jobs), workers).unwrap_err();
            assert!(
                error.message.contains("f012.txt"),
                "workers {workers}, attempt {attempt}: {}",
                error.message
            );
            fs::remove_dir_all(&dist).unwrap();
        }
    }
}

#[test]
fn a_failure_does_not_stop_the_files_of_the_other_lanes() {
    let dist = scratch("others-continue");
    let files: Vec<OutputFile> = (0..200)
        .map(|n| file(&format!("f{n:03}.txt"), "x"))
        .collect();
    block(&dist, "f010.txt");
    let jobs = jobs(&dist, &files);
    assert!(write_lanes(&jobs, &plan_lanes(&jobs), 8).is_err());
    for n in (0..200).filter(|n| *n != 10) {
        assert!(
            dist.join(format!("f{n:03}.txt")).is_file(),
            "f{n:03}.txt was not written"
        );
    }
    assert!(
        !snapshot(&dist)
            .keys()
            .any(|name| name.ends_with(".anaquel-tmp")),
        "a failed rename left its temporary file"
    );
    fs::remove_dir_all(&dist).unwrap();
}

#[test]
fn a_file_that_is_also_a_folder_is_refused_in_either_order_and_in_any_case() {
    for set in [
        vec![file("a", "1"), file("a/b", "2")],
        vec![file("a/b", "2"), file("a", "1")],
        vec![file("A", "1"), file("a/b", "2")],
        vec![file("x/y.txt", "1"), file("x/y.txt/z", "2")],
        vec![file("a", "1"), file("a./b", "2")],
        vec![file("a ", "1"), file("a/b", "2")],
        vec![file("x/a:stream", "1"), file("x/a/b", "2")],
    ] {
        let error = refuse_file_folder_conflicts(&set).unwrap_err();
        assert!(
            error.message.contains("cannot be a folder"),
            "{}",
            error.message
        );
    }
}

#[test]
fn names_that_only_share_a_beginning_are_not_a_conflict() {
    let set = [
        file("a/b", "1"),
        file("a/c", "2"),
        file("ab", "3"),
        file("a/b.txt", "4"),
        file("...", "5"),
        file("a/...", "6"),
    ];
    refuse_file_folder_conflicts(&set).unwrap();
    refuse_file_folder_conflicts(&[]).unwrap();
}

// ----- the blocks of lanes -----------------------------------------------------------------------

/// Every run a queue hands out, taken by one thread until it answers `None`.
fn drain(queue: &BlockQueue) -> Vec<Range<usize>> {
    std::iter::from_fn(|| queue.claim()).collect()
}

#[test]
fn a_block_is_a_quarter_of_a_writers_share_and_never_empty() {
    // (lanes, workers, block)
    for (lanes, workers, expected) in [
        (0, 8, 1),
        (1, 8, 1),
        (CONCURRENCY_THRESHOLD, 8, 2),
        (600, 8, 19),
        (10_377, 8, 325),
        (10_377, 4, 649),
        (100, 1, 25),
        (7, 0, 2),
        (100, usize::MAX, 1),
    ] {
        assert_eq!(
            BlockQueue::new(lanes, workers).block,
            expected,
            "{lanes} lanes, {workers} workers"
        );
    }
}

#[test]
fn the_claims_tile_every_lane_once_in_order_and_then_stop() {
    for lanes in [0, 1, 2, 7, 63, 64, 65, 600, 10_377] {
        for workers in [1, 2, 3, 8] {
            let queue = BlockQueue::new(lanes, workers);
            let claims = drain(&queue);
            let mut expected_start = 0;
            for claim in &claims {
                assert_eq!(
                    claim.start, expected_start,
                    "{lanes} lanes, {workers} workers"
                );
                assert!(!claim.is_empty() && claim.len() <= queue.block);
                expected_start = claim.end;
            }
            assert_eq!(expected_start, lanes, "{lanes} lanes, {workers} workers");
            assert!(queue.claim().is_none() && queue.claim().is_none());
        }
    }
}

#[test]
fn only_the_last_block_may_be_shorter_than_the_others() {
    let queue = BlockQueue::new(10_377, 8);
    let claims = drain(&queue);
    let (last, rest) = claims.split_last().unwrap();
    assert!(rest.iter().all(|claim| claim.len() == queue.block));
    assert_eq!(last.len(), 10_377 - rest.len() * queue.block);
}

#[test]
fn a_writer_claims_a_few_runs_and_not_one_lane_at_a_time() {
    // The point of the blocks: far fewer claims than lanes, so a writer stays in a few folders.
    let queue = BlockQueue::new(10_000, MAX_WRITERS);
    let claims = drain(&queue).len();
    assert!(
        claims <= MAX_WRITERS * BLOCKS_PER_WRITER,
        "{claims} claims for 10 000 lanes"
    );
    assert!(
        claims >= MAX_WRITERS,
        "every writer gets at least one: {claims}"
    );
}

#[test]
fn eight_threads_claiming_at_once_get_disjoint_runs_that_tile_the_lanes() {
    for _ in 0..20 {
        let queue = BlockQueue::new(10_377, 8);
        let mut claims: Vec<Range<usize>> = thread::scope(|scope| {
            let handles: Vec<_> = (0..8).map(|_| scope.spawn(|| drain(&queue))).collect();
            handles
                .into_iter()
                .flat_map(|handle| handle.join().unwrap())
                .collect()
        });
        claims.sort_by_key(|claim| claim.start);
        let mut next = 0;
        for claim in &claims {
            assert_eq!(claim.start, next, "a gap or an overlap before {claim:?}");
            next = claim.end;
        }
        assert_eq!(next, 10_377);
    }
}

#[test]
fn no_writer_count_below_two_leaves_a_file_unwritten() {
    // "The calling thread when `workers` is 1 or less": zero must not mean no thread at all.
    for workers in [0, 1] {
        let dist = scratch(&format!("workers-{workers}"));
        let files = many_files(100);
        let jobs = jobs(&dist, &files);
        write_lanes(&jobs, &plan_lanes(&jobs), workers).unwrap();
        assert_eq!(snapshot(&dist).len(), files.len(), "{workers} workers");
        fs::remove_dir_all(&dist).unwrap();
    }
}

#[test]
fn just_above_the_threshold_every_file_is_still_written_whole() {
    // 65 lanes and eight writers: blocks of three, the last one with two lanes.
    let dist = scratch("just-above-threshold");
    let files = many_files(CONCURRENCY_THRESHOLD + 1);
    let jobs = jobs(&dist, &files);
    write_lanes(&jobs, &plan_lanes(&jobs), 8).unwrap();
    let written = snapshot(&dist);
    assert_eq!(written.len(), files.len());
    for output in &files {
        assert_eq!(written[&output.path], output.bytes, "{}", output.path);
    }
    fs::remove_dir_all(&dist).unwrap();
}

#[test]
fn a_failure_inside_a_block_does_not_stop_the_lanes_after_it_in_that_block() {
    // 200 lanes and eight writers give blocks of seven: the failing lane 10 has lanes 7 to 13 around it.
    let dist = scratch("failure-in-block");
    let files: Vec<OutputFile> = (0..200)
        .map(|n| file(&format!("f{n:03}.txt"), "x"))
        .collect();
    block(&dist, "f010.txt");
    let jobs = jobs(&dist, &files);
    let lanes = plan_lanes(&jobs);
    assert_eq!(BlockQueue::new(lanes.len(), 8).block, 7);
    assert!(write_lanes(&jobs, &lanes, 8).is_err());
    for n in 7..14 {
        assert_eq!(
            dist.join(format!("f{n:03}.txt")).is_file(),
            n != 10,
            "f{n:03}.txt"
        );
    }
    fs::remove_dir_all(&dist).unwrap();
}
