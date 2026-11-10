//! Run a test that calls `fork()` in a process of its own.
//!
//! libtest runs tests on parallel threads. `fork()` copies only the calling
//! thread, so a child forked while another test thread holds a lock (lcms2's
//! `_cmsContextPoolHeadMutex`, the stderr lock, ...) inherits that lock held
//! by a thread that no longer exists. The child blocks forever and the
//! parent's `waitpid` blocks with it. `c_parity_types4::alloc_fail_arms` hung
//! this way for days.
//!
//! [`delegated`] re-runs the named test alone (`--exact --test-threads=1`) in a
//! fresh copy of the test binary, where no other test thread exists, and
//! asserts that run passed.

use std::process::Command;

const KEY: &str = "LCMS_TEST_SOLO";

/// Returns `true` once `name` has run and passed in its own process; the
/// caller then returns. Returns `false` inside that process, where the caller
/// runs its body and may fork safely.
pub fn delegated(name: &str) -> bool {
    if std::env::var(KEY).is_ok_and(|v| v == name) {
        return false;
    }
    let exe = std::env::current_exe().expect("test binary path");
    let out = Command::new(exe)
        .args([name, "--exact", "--test-threads=1", "--nocapture"])
        .env(KEY, name)
        .output()
        .expect("spawn test binary");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    print!("{stdout}");
    eprint!("{stderr}");
    assert!(out.status.success(), "{name} in its own process: {}", out.status);
    assert!(
        stdout.contains("1 passed"),
        "{name} in its own process ran no test (filter mismatch?)"
    );
    true
}
