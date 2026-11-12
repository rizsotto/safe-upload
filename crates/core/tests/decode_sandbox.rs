//! The core decoder forces bwrap. With `SandboxSelector::Auto`, this
//! environment variable makes glycin decode without any sandbox; the core
//! must still use bwrap. This file holds one test, because the variable
//! is read once per process.

mod common;

use safe_upload_core::Limits;
use safe_upload_core::decode::GlycinDecoder;

#[test]
fn bwrap_is_used_even_when_auto_would_drop_the_sandbox() {
    // SAFETY: the only test in this binary, set before glycin starts.
    unsafe { std::env::set_var("GLYCIN_DISABLE_SANDBOX", "i-know-the-risks") };
    let jpeg = common::fixture("orient-1.jpg");
    // The decoder returns an error if glycin ran without bwrap.
    GlycinDecoder
        .decode_bytes(jpeg, &Limits::default())
        .unwrap();
}
