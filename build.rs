//! Build script: stamps a UTC build identifier (MMDD-HHMM) into the binary
//! so the status bar can show which build is running during development.
//!
//! No `rerun-if` directive is emitted on purpose: without one, cargo reruns
//! this script whenever any file in the package changes, which is exactly
//! the freshness we want for a build stamp.

use std::time::{SystemTime, UNIX_EPOCH};

/// Days-since-epoch to (year, month, day) — Howard Hinnant's civil-from-days.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (yoe as i64 + era * 400 + if m <= 2 { 1 } else { 0 }, m, d)
}

fn main() {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before 1970")
        .as_secs() as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400) as u32;
    let (_, month, day) = civil_from_days(days);
    let (hour, minute) = (rem / 3600, (rem % 3600) / 60);
    println!(
        "cargo:rustc-env=EDITORMINI_BUILD_ID={month:02}{day:02}-{hour:02}{minute:02}"
    );
}
