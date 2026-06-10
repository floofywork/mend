//! Frozen acceptance tests for T-04.01 — Define parsed-edit model.
//!
//! The criteria are about a Rust *type* in the `mend` crate — the shared currency
//! every response parser emits and the patch engine consumes. Exactly as the
//! prompt/context/error tests do, the honest way to observe a crate API is to
//! *compile and run a tiny probe program against the real `mend` crate*. The
//! probes pin the contract the criteria imply; this test crate stays std-only and
//! never references the not-yet-existing type directly, so it always compiles.
//! Redness comes from each probe failing to *build* (the `ParsedEdit` type does
//! not exist yet), not from this file failing to parse. Each probe is built into
//! an isolated `CARGO_TARGET_DIR` and run `--offline`, so it neither contends with
//! the outer build lock nor touches the network.
//!
//! The probes pin the minimal contract the criteria imply — a two-variant value
//! type collected into a `Vec`:
//!   - `ParsedEdit::SearchReplace { search: String, replace: String }` — a
//!     targeted (search_text, replace_text) pair.
//!   - `ParsedEdit::WholeFile { body: String }` — a whole-file replacement body.
//!   - `Vec<ParsedEdit>` — a multi-hunk response, order preserved.
//!   - `ParsedEdit` derives `PartialEq` (for `==`/`!=` assertions across equal,
//!     field-differing, and variant-differing values) and `Debug` (for `{:?}`).
//!
//!   C1: a `ParsedEdit` type represents either a (search_text, replace_text) pair or a whole-file body.
//!   C2: a `Vec<ParsedEdit>` represents a multi-hunk response.
//!   C3: `ParsedEdit` derives `PartialEq` and `Debug` for test assertions.

use std::path::PathBuf;
use std::process::Command;

/// The directory holding the `mend` crate's `Cargo.toml`.
fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The cargo executable cargo itself ran us with, falling back to `cargo`.
fn cargo_bin() -> String {
    option_env!("CARGO").unwrap_or("cargo").to_string()
}

/// A fresh, process-unique scratch directory under the system temp dir.
fn unique_temp_dir(tag: &str) -> PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut dir = std::env::temp_dir();
    dir.push(format!("mend-parsed-edit-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Outcome of building and running a probe: whether it built-and-exited-0, plus
/// the combined stdout+stderr for diagnostics.
struct Probe {
    ran_ok: bool,
    output: String,
}

/// Build and run a probe crate whose `main.rs` is `main_src`, against the real
/// `mend` crate via a path dependency. A standalone `[workspace]` keeps the
/// probe from inheriting any ambient workspace; an isolated target dir keeps it
/// off the outer build lock; `--offline` keeps it off the network.
///
/// In the red state `mend` has no `ParsedEdit` type, so the probe fails to
/// resolve and `ran_ok` is false. Once the `mend` library exposes the type the
/// probe references, it builds, runs, and prints its success sentinel. A probe
/// that panics (e.g. on a broken invariant) exits non-zero and prints no
/// sentinel, so a panic fails the corresponding test.
fn run_probe(tag: &str, main_src: &str) -> Probe {
    let crate_dir = unique_temp_dir(tag);
    let target_dir = unique_temp_dir(&format!("{tag}-target"));

    // Literal (single-quoted) TOML string so backslashes in the path are not
    // treated as escapes on any platform.
    let manifest = format!(
        "[package]\n\
         name = \"mend_parsed_edit_probe\"\n\
         version = \"0.0.0\"\n\
         edition = \"2021\"\n\
         \n\
         [dependencies]\n\
         mend = {{ path = '{}' }}\n\
         \n\
         [[bin]]\n\
         name = \"probe\"\n\
         path = \"src/main.rs\"\n\
         \n\
         [workspace]\n",
        manifest_dir().display()
    );
    std::fs::write(crate_dir.join("Cargo.toml"), manifest).expect("write probe Cargo.toml");
    std::fs::create_dir_all(crate_dir.join("src")).expect("create probe src dir");
    std::fs::write(crate_dir.join("src").join("main.rs"), main_src).expect("write probe main.rs");

    let out = Command::new(cargo_bin())
        .current_dir(&crate_dir)
        .env("CARGO_TARGET_DIR", &target_dir)
        .args(["run", "--quiet", "--offline", "--bin", "probe"])
        .output()
        .expect("failed to spawn `cargo run` for probe");

    let output = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let ran_ok = out.status.success();

    let _ = std::fs::remove_dir_all(&crate_dir);
    let _ = std::fs::remove_dir_all(&target_dir);

    Probe { ran_ok, output }
}

/// C1 — a `ParsedEdit` represents EITHER a (search_text, replace_text) pair OR a
/// whole-file body. The probe constructs one value of each shape, then pattern
/// matches each to recover its data: the `SearchReplace` value must carry both the
/// search and the replace text it was built with, and the `WholeFile` value must
/// carry its body. A type that could not represent both alternatives — or that
/// dropped one of the search-replace halves — would fail to build or fail the
/// `assert_eq!`s.
#[test]
fn c1_parsededit_represents_pair_or_whole_file() {
    let src = r##"
fn main() {
    let pair = mend::ParsedEdit::SearchReplace {
        search: "OLD_SEARCH_TEXT".to_string(),
        replace: "NEW_REPLACE_TEXT".to_string(),
    };
    let whole = mend::ParsedEdit::WholeFile {
        body: "WHOLE_FILE_BODY".to_string(),
    };

    match &pair {
        mend::ParsedEdit::SearchReplace { search, replace } => {
            assert_eq!(search, "OLD_SEARCH_TEXT", "C1: search half must be preserved");
            assert_eq!(replace, "NEW_REPLACE_TEXT", "C1: replace half must be preserved");
        }
        _ => panic!("C1: a (search, replace) pair must be the SearchReplace variant"),
    }
    match &whole {
        mend::ParsedEdit::WholeFile { body } => {
            assert_eq!(body, "WHOLE_FILE_BODY", "C1: whole-file body must be preserved");
        }
        _ => panic!("C1: a whole-file body must be the WholeFile variant"),
    }

    println!("C1_OK");
}
"##;
    let p = run_probe("c1", src);
    assert!(
        p.ran_ok && p.output.contains("C1_OK"),
        "ParsedEdit must represent either a (search, replace) pair or a whole-file body; probe output:\n{}",
        p.output
    );
}

/// C2 — a `Vec<ParsedEdit>` represents a multi-hunk response. The probe builds a
/// vector mixing both variants, then asserts the length matches the number of
/// hunks and that each element is exactly the edit placed at that index, in the
/// same order — pinning "multi-hunk" as an ordered collection rather than a single
/// edit. (Element equality leans on `PartialEq`, which C3 pins independently.)
#[test]
fn c2_vec_parsededit_is_multi_hunk_response() {
    let src = r##"
fn main() {
    let edits: Vec<mend::ParsedEdit> = vec![
        mend::ParsedEdit::SearchReplace { search: "A".to_string(), replace: "B".to_string() },
        mend::ParsedEdit::WholeFile { body: "C".to_string() },
        mend::ParsedEdit::SearchReplace { search: "D".to_string(), replace: "E".to_string() },
    ];

    assert_eq!(edits.len(), 3, "C2: a Vec<ParsedEdit> must hold every hunk");
    assert_eq!(
        edits[0],
        mend::ParsedEdit::SearchReplace { search: "A".to_string(), replace: "B".to_string() },
        "C2: hunk 0 must be preserved in order"
    );
    assert_eq!(
        edits[1],
        mend::ParsedEdit::WholeFile { body: "C".to_string() },
        "C2: hunk 1 must be preserved in order"
    );
    assert_eq!(
        edits[2],
        mend::ParsedEdit::SearchReplace { search: "D".to_string(), replace: "E".to_string() },
        "C2: hunk 2 must be preserved in order"
    );

    println!("C2_OK");
}
"##;
    let p = run_probe("c2", src);
    assert!(
        p.ran_ok && p.output.contains("C2_OK"),
        "Vec<ParsedEdit> must represent a multi-hunk response in order; probe output:\n{}",
        p.output
    );
}

/// C3 — `ParsedEdit` derives `PartialEq` and `Debug`. The probe exercises both:
/// for `PartialEq` it checks all three relations — two field-identical values are
/// `==`, two values differing only in a field are `!=`, and two different variants
/// are `!=`; for `Debug` it formats a value with `{:?}` and checks the rendering
/// names the variant and includes its field data. A type missing either derive
/// would fail to build.
#[test]
fn c3_parsededit_derives_partialeq_and_debug() {
    let src = r##"
fn main() {
    let a = mend::ParsedEdit::SearchReplace { search: "S".to_string(), replace: "R".to_string() };
    let a_same = mend::ParsedEdit::SearchReplace { search: "S".to_string(), replace: "R".to_string() };
    let a_diff = mend::ParsedEdit::SearchReplace { search: "S".to_string(), replace: "DIFFERENT".to_string() };
    let other_variant = mend::ParsedEdit::WholeFile { body: "S".to_string() };

    // PartialEq: equal, field-differing, and variant-differing relations.
    assert!(a == a_same, "C3: field-identical ParsedEdits must compare equal");
    assert!(a != a_diff, "C3: ParsedEdits differing in a field must compare unequal");
    assert!(a != other_variant, "C3: ParsedEdits of different variants must compare unequal");

    // Debug: must render the variant name and the field data.
    let dbg = format!("{:?}", a);
    assert!(dbg.contains("SearchReplace"), "C3: Debug must render the variant name, got: {:?}", dbg);
    assert!(
        dbg.contains("S") && dbg.contains("R"),
        "C3: Debug must render the field data, got: {:?}",
        dbg
    );

    println!("C3_OK");
}
"##;
    let p = run_probe("c3", src);
    assert!(
        p.ran_ok && p.output.contains("C3_OK"),
        "ParsedEdit must derive PartialEq and Debug; probe output:\n{}",
        p.output
    );
}
