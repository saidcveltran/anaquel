//! Runs every fuzz check over its seed files and over mutations of them, on every system.
//!
//! This is not fuzzing: there is no coverage feedback, and the mutations are few. It is a cheap
//! regression net. A file that `cargo fuzz` finds to crash a target belongs in
//! `fuzz/corpus/<target>/`, and from then on this test runs it on every push.

use std::fs;
use std::path::PathBuf;

const MUTATIONS_PER_SEED: usize = 300;
const INTERESTING: &[u8] = b":\n\"'<>&#%-`[]()\\/=!{}|;@ \t\r.?*_~+,$";
const WORDS: [&[u8]; 9] = [
    b":::hint\n",
    b"\n:::\n",
    b"javascript:",
    b"<script>",
    b"\n---\n",
    b"%",
    b"::::tabs\n",
    b"\xe2\x80\xae",
    b"\xff",
];

struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, limit: usize) -> usize {
        let limit = u64::try_from(limit.max(1)).unwrap_or(1);
        usize::try_from(self.next() % limit).unwrap_or(0)
    }
}

fn mutate(seed: &[u8], random: &mut Random) -> Vec<u8> {
    let mut data = seed.to_vec();
    for _ in 0..=random.below(4) {
        match random.below(6) {
            0 if !data.is_empty() => {
                let at = random.below(data.len());
                data[at] ^= 1 << random.below(8);
            }
            1 => {
                let at = random.below(data.len() + 1);
                data.insert(at, INTERESTING[random.below(INTERESTING.len())]);
            }
            2 if !data.is_empty() => {
                let at = random.below(data.len());
                let end = (at + 1 + random.below(16)).min(data.len());
                data.drain(at..end);
            }
            3 if !data.is_empty() => {
                let at = random.below(data.len());
                let end = (at + 1 + random.below(64)).min(data.len());
                let piece = data[at..end].to_vec();
                let to = random.below(data.len() + 1);
                data.splice(to..to, piece);
            }
            4 => data.truncate(random.below(data.len() + 1)),
            _ => {
                let at = random.below(data.len() + 1);
                data.splice(at..at, WORDS[random.below(WORDS.len())].iter().copied());
            }
        }
    }
    data
}

fn seeds(target: &str) -> Vec<(String, Vec<u8>)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fuzz/corpus")
        .join(target);
    let listing = fs::read_dir(&dir);
    assert!(listing.is_ok(), "{}: cannot be read", dir.display());
    let mut found: Vec<_> = listing
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let bytes = fs::read(e.path()).unwrap_or_default();
            (name, bytes)
        })
        .collect();
    found.sort();
    assert!(
        found.len() >= 3,
        "{target}: only {} seed files",
        found.len()
    );
    found
}

fn run(target: &str, check: fn(&[u8])) {
    let mut random = Random(0x9e37_79b9_7f4a_7c15);
    for (name, seed) in seeds(target) {
        check(&seed);
        for round in 0..MUTATIONS_PER_SEED {
            let input = mutate(&seed, &mut random);
            let outcome = std::panic::catch_unwind(|| check(&input));
            assert!(
                outcome.is_ok(),
                "{target}: seed {name}, round {round} failed on {:?}",
                String::from_utf8_lossy(&input)
            );
        }
    }
}

#[test]
fn directive_grammar_holds_on_seeds_and_mutations() {
    run("directive_grammar", anaquel_fuzzcheck::directive_grammar);
}

#[test]
fn front_matter_holds_on_seeds_and_mutations() {
    run("front_matter", anaquel_fuzzcheck::front_matter);
}

#[test]
fn url_policy_holds_on_seeds_and_mutations() {
    run("url_policy", anaquel_fuzzcheck::url_policy);
}

#[test]
fn link_resolver_holds_on_seeds_and_mutations() {
    run("link_resolver", anaquel_fuzzcheck::link_resolver);
}
