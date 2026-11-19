use super::*;
use crate::scratch::Scratch;

fn time(text: &str) -> Timestamp {
    Timestamp::parse(text).unwrap()
}

/// An environment lookup over a fixed list of variables.
fn environment(variables: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> + use<> {
    let variables: Vec<(String, String)> = variables
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect();
    move |name| {
        variables
            .iter()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, value)| OsString::from(value))
    }
}

#[test]
fn on_windows_the_folder_is_under_the_local_application_data() {
    let lookup = environment(&[("LOCALAPPDATA", r"C:\Users\x\AppData\Local")]);
    assert_eq!(
        state_dir(Platform::Windows, &lookup),
        Some(PathBuf::from(r"C:\Users\x\AppData\Local").join("anaquel"))
    );
}

#[test]
fn on_windows_the_profile_is_the_fallback() {
    let lookup = environment(&[("USERPROFILE", r"C:\Users\x")]);
    assert_eq!(
        state_dir(Platform::Windows, &lookup),
        Some(
            PathBuf::from(r"C:\Users\x")
                .join("AppData")
                .join("Local")
                .join("anaquel")
        )
    );
}

#[test]
fn on_unix_the_xdg_folder_wins_over_home() {
    // A path is absolute on Windows only with a drive, so the root is chosen per system.
    let root = if cfg!(windows) { r"C:\" } else { "/" };
    let state = format!("{root}state");
    let home = format!("{root}home");
    let lookup = environment(&[("XDG_STATE_HOME", &state), ("HOME", &home)]);
    assert_eq!(
        state_dir(Platform::Unix, &lookup),
        Some(PathBuf::from(&state).join("anaquel"))
    );
    let lookup = environment(&[("HOME", &home)]);
    assert_eq!(
        state_dir(Platform::Unix, &lookup),
        Some(
            PathBuf::from(&home)
                .join(".local")
                .join("state")
                .join("anaquel")
        )
    );
}

#[test]
fn a_variable_that_is_empty_or_relative_is_ignored() {
    // Rooted paths are absolute on Unix; on Windows they also need a drive, so the tests that
    // follow only use values that are absolute on the system that runs them.
    let absolute_home = if cfg!(windows) {
        r"C:\home\x"
    } else {
        "/home/x"
    };
    let lookup = environment(&[
        ("XDG_STATE_HOME", "relative/state"),
        ("HOME", absolute_home),
    ]);
    let found = state_dir(Platform::Unix, &lookup).unwrap();
    assert!(found.starts_with(absolute_home), "{found:?}");
    assert!(found.ends_with("anaquel"));

    let lookup = environment(&[("XDG_STATE_HOME", ""), ("HOME", "")]);
    assert_eq!(state_dir(Platform::Unix, &lookup), None);
    let lookup = environment(&[("LOCALAPPDATA", "AppData"), ("USERPROFILE", "..")]);
    assert_eq!(state_dir(Platform::Windows, &lookup), None);
    assert_eq!(state_dir(Platform::Unix, &environment(&[])), None);
}

#[test]
fn nothing_is_recorded_before_the_first_manifest() {
    let scratch = Scratch::new("none");
    assert_eq!(read_last_seen(scratch.root()).unwrap(), None);
    // A folder that does not exist yet is the same as an empty one.
    assert_eq!(read_last_seen(&scratch.path("absent")).unwrap(), None);
}

#[test]
fn the_first_time_is_written_and_read_back() {
    let scratch = Scratch::new("first");
    let dir = scratch.path("state").join("anaquel");
    assert!(record_last_seen(&dir, &time("2026-11-20T00:00:00Z")).unwrap());
    assert_eq!(
        read_last_seen(&dir).unwrap(),
        Some(time("2026-11-20T00:00:00Z"))
    );
    assert_eq!(
        fs::read_to_string(dir.join(LAST_SEEN_FILE)).unwrap(),
        "2026-11-20T00:00:00Z\n"
    );
}

#[test]
fn a_newer_time_replaces_the_stored_one_and_an_older_or_equal_one_does_not() {
    let scratch = Scratch::new("monotone");
    let dir = scratch.root();
    assert!(record_last_seen(dir, &time("2026-11-20T00:00:00Z")).unwrap());
    assert!(record_last_seen(dir, &time("2026-12-01T00:00:00Z")).unwrap());

    assert!(!record_last_seen(dir, &time("2026-11-25T00:00:00Z")).unwrap());
    assert!(!record_last_seen(dir, &time("2026-12-01T00:00:00Z")).unwrap());
    assert_eq!(
        read_last_seen(dir).unwrap(),
        Some(time("2026-12-01T00:00:00Z"))
    );
    // No temporary file is left behind by any of these.
    assert_eq!(scratch.names(), [LAST_SEEN_FILE]);
}

#[test]
fn a_file_that_is_not_a_time_is_reported_not_replaced() {
    let scratch = Scratch::new("corrupt");
    let file = scratch.path(LAST_SEEN_FILE);
    for content in [
        &b"yesterday\n"[..],
        b"",
        b"2026-11-20\n",
        &[0xff, 0xfe, 0x00][..],
        &[b'x'; 200][..],
    ] {
        fs::write(&file, content).unwrap();
        let error = read_last_seen(scratch.root()).unwrap_err();
        assert!(matches!(error, StateError::Corrupt { .. }), "{error}");
        // Recording must not paper over a damaged file by writing a new one.
        assert!(record_last_seen(scratch.root(), &time("2027-01-01T00:00:00Z")).is_err());
        assert_eq!(fs::read(&file).unwrap(), content);
    }
}

#[test]
fn a_state_file_that_cannot_be_opened_is_an_error_not_an_empty_state() {
    // A folder where the file should be: it cannot be read as a time, and treating it as
    // "nothing recorded" would switch off the protection against an old manifest.
    let scratch = Scratch::new("unreadable");
    fs::create_dir(scratch.path(LAST_SEEN_FILE)).unwrap();
    let error = read_last_seen(scratch.root()).unwrap_err();
    assert!(matches!(error, StateError::Io { .. }), "{error}");
}

#[test]
fn a_file_of_exactly_the_largest_size_is_still_read() {
    let scratch = Scratch::new("limit");
    let mut text = String::from("2026-11-20T00:00:00Z");
    text.push_str(&" ".repeat(usize::try_from(MAX_FILE_BYTES).unwrap() - text.len()));
    assert_eq!(text.len() as u64, MAX_FILE_BYTES);
    fs::write(scratch.path(LAST_SEEN_FILE), &text).unwrap();
    assert_eq!(
        read_last_seen(scratch.root()).unwrap(),
        Some(time("2026-11-20T00:00:00Z"))
    );

    fs::write(scratch.path(LAST_SEEN_FILE), format!("{text} ")).unwrap();
    assert!(matches!(
        read_last_seen(scratch.root()),
        Err(StateError::Corrupt { .. })
    ));
}

#[test]
fn a_state_path_that_is_a_file_is_an_error_not_a_panic() {
    let scratch = Scratch::new("blocked");
    fs::write(scratch.path("state"), b"a file where the folder should be").unwrap();
    let error =
        record_last_seen(&scratch.path("state"), &time("2026-11-20T00:00:00Z")).unwrap_err();
    assert!(matches!(error, StateError::Io { .. }), "{error}");
}
