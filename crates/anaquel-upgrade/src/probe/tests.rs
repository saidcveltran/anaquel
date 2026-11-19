use std::path::PathBuf;

use super::*;

fn v(text: &str) -> Version {
    Version::parse(text).unwrap()
}

#[test]
fn the_answer_of_a_program_must_be_anaquel_and_a_version() {
    assert_eq!(check_answer("anaquel 0.2.0\n", None), Ok(()));
    assert_eq!(check_answer("anaquel 0.2.0-rc.1\r\n", None), Ok(()));
    for bad in [
        "",
        "\n",
        "0.2.0",
        "anaquel",
        "anaquel ",
        "anaquel x.y.z",
        "anaquel 0.2",
        "another-tool 0.2.0",
        "ANAQUEL 0.2.0",
        "anaquel 0.2.0 and more words",
        "error: unknown command",
    ] {
        assert!(check_answer(bad, None).is_err(), "{bad:?} must be refused");
    }
}

#[test]
fn the_version_must_be_the_one_expected_when_one_is_expected() {
    assert_eq!(check_answer("anaquel 0.2.0\n", Some(&v("0.2.0"))), Ok(()));
    let error = check_answer("anaquel 0.2.1\n", Some(&v("0.2.0"))).unwrap_err();
    assert_eq!(error, "it reports version 0.2.1 but 0.2.0 was expected");
    // A version that differs only in its build metadata or prerelease is another version.
    assert!(check_answer("anaquel 0.2.0-rc.1\n", Some(&v("0.2.0"))).is_err());
}

#[test]
fn a_program_that_does_not_exist_is_reported_as_not_started() {
    let missing = PathBuf::from("this").join("file").join("does-not-exist");
    let error = run_version_probe(&missing, None).unwrap_err();
    assert!(error.starts_with("it could not be started: "), "{error}");
}
