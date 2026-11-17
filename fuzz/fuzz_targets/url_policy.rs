#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    anaquel_fuzzcheck::url_policy(data);
});
