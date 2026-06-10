//! Frozen acceptance tests for T-06.03 — Emit JSON result.
//!
//! The criteria describe a Rust API: the machine-readable output mode that turns a
//! result (path, original text, proposed text, the edit list, and an `applied`
//! flag) into one JSON object, and back again. So — exactly as the T-06.02
//! render-diff, T-06.01 compute-unified-diff, T-05.04 apply and T-04.01
//! parsed-edit tests do — the honest way to observe the criteria is to *compile and
//! run a tiny probe program against the real `mend` crate*. The probes pin the
//! contract the criteria imply; this test crate stays std-only and never references
//! the not-yet-existing API directly, so it always compiles. Redness comes from
//! each probe failing to *build* (the JSON-result API does not exist yet), not from
//! this file failing to parse. Each probe is built into an isolated
//! `CARGO_TARGET_DIR` and run `--offline`, so it neither contends with the outer
//! build lock nor touches the network.
//!
//! The probes pin the minimal, idiomatic contract the criteria imply:
//!   - `mend::JsonResult { path: String, original: String, proposed: String,
//!     edits: Vec<mend::ParsedEdit>, applied: bool }` — the result struct, deriving
//!     `Debug` and `PartialEq` for round-trip assertions.
//!   - `mend::emit_json(result: &mend::JsonResult) -> String` — serialises the
//!     result to one JSON object (no colour, no human diff, no streaming — all out
//!     of scope).
//!   - `mend::parse_json(json: &str) -> mend::Result<mend::JsonResult>` — parses the
//!     emitted JSON back into the result struct.
//!   - C1: the JSON carries the path, the original text, the proposed text, and the
//!     edit list — every input field is present in the serialised string.
//!   - C2: the JSON includes an `applied` boolean that reflects the input flag —
//!     pinned on BOTH sides (true serialises/round-trips true, false serialises/
//!     round-trips false), so a mutant that hardcodes or inverts the flag dies.
//!   - C3: emitting then parsing reconstructs the exact same structure, field for
//!     field; re-serialising the parsed struct reproduces the identical JSON.
//!
//!   C1: serializes path, original text, proposed text, and the edit list to JSON.
//!   C2: the JSON includes an `applied` boolean reflecting whether changes were written.
//!   C3: the emitted JSON parses back into the same structure.

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
    dir.push(format!(
        "mend-json-result-{tag}-{}-{nanos}",
        std::process::id()
    ));
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
/// In the red state `mend` has no JSON-result API, so the probe fails to resolve
/// and `ran_ok` is false. Once the `mend` library exposes the symbols the probe
/// references, it builds, runs, and prints its success sentinel. A probe that
/// panics (e.g. on a broken invariant) exits non-zero and prints no sentinel, so a
/// panic fails the corresponding test.
fn run_probe(tag: &str, main_src: &str) -> Probe {
    let crate_dir = unique_temp_dir(tag);
    let target_dir = unique_temp_dir(&format!("{tag}-target"));

    // Literal (single-quoted) TOML string so backslashes in the path are not
    // treated as escapes on any platform.
    let manifest = format!(
        "[package]\n\
         name = \"mend_json_result_probe\"\n\
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

/// C1 — `emit_json` serialises the path, the original text, the proposed text, and
/// the edit list into one JSON string. The probe builds a result whose every field
/// carries a distinct, unmistakable marker (so a field's marker can only appear in
/// the output if that field was actually serialised — the edit markers do not occur
/// in the path/original/proposed text), then asserts the JSON is non-empty, names
/// each schema key (`path`, `original`, `proposed`, `edits`), and contains every
/// field's value. A mutant that drops or omits any of the four serialised fields
/// loses the corresponding marker and the probe fails. Round-tripping the JSON back
/// into an equal struct confirms the string is genuine, parseable JSON rather than
/// an arbitrary blob.
#[test]
fn c1_serializes_path_original_proposed_and_edits() {
    let src = r##"
fn main() {
    let result = mend::JsonResult {
        path: "src/lib.rs".to_string(),
        original: "ALPHA_ORIGINAL".to_string(),
        proposed: "BETA_PROPOSED".to_string(),
        edits: vec![
            mend::ParsedEdit::SearchReplace {
                search: "SEARCH_NEEDLE".to_string(),
                replace: "REPLACE_NEEDLE".to_string(),
            },
            mend::ParsedEdit::WholeFile { body: "WHOLEFILE_NEEDLE".to_string() },
        ],
        applied: false,
    };

    let json = mend::emit_json(&result);
    assert!(!json.is_empty(), "C1: emitted JSON must be non-empty, got {:?}", json);

    // Every schema key and every field value is present in the serialised string.
    for needle in [
        "path", "src/lib.rs",
        "original", "ALPHA_ORIGINAL",
        "proposed", "BETA_PROPOSED",
        "edits", "SEARCH_NEEDLE", "REPLACE_NEEDLE", "WHOLEFILE_NEEDLE",
    ] {
        assert!(
            json.contains(needle),
            "C1: emitted JSON must contain {:?}, got {:?}",
            needle, json
        );
    }

    // It is genuine JSON: it parses back into an equal structure.
    let parsed = mend::parse_json(&json).expect("C1: emitted JSON must parse");
    assert_eq!(
        parsed, result,
        "C1: parsed JSON must reconstruct the serialised result, got {:?}",
        parsed
    );

    println!("C1_OK");
}
"##;
    let p = run_probe("c1", src);
    assert!(
        p.ran_ok && p.output.contains("C1_OK"),
        "emit_json must serialise path, original, proposed, and the edit list; probe output:\n{}",
        p.output
    );
}

/// C2 — the JSON includes an `applied` boolean reflecting whether changes were
/// written. The probe pins BOTH sides of the flag on otherwise-identical results:
/// with `applied = true` the JSON names the `applied` key, carries a `true` literal,
/// carries no `false`, and round-trips back to `applied == true`; with
/// `applied = false` it carries a `false` literal, no `true`, and round-trips back
/// to `applied == false`. The field values are single letters that contain neither
/// "true" nor "false" as substrings, so the literal checks are unambiguous.
/// Asserting both sides kills a mutant that hardcodes the flag to a constant and one
/// that inverts it.
#[test]
fn c2_includes_applied_boolean() {
    let src = r##"
fn main() {
    fn make(applied: bool) -> mend::JsonResult {
        mend::JsonResult {
            path: "p".to_string(),
            original: "o".to_string(),
            proposed: "n".to_string(),
            edits: vec![mend::ParsedEdit::WholeFile { body: "b".to_string() }],
            applied,
        }
    }

    // applied = true: JSON has the key, a `true` literal, no `false`, round-trips true.
    let json_true = mend::emit_json(&make(true));
    assert!(
        json_true.contains("applied"),
        "C2: JSON must include an `applied` key, got {:?}", json_true
    );
    assert!(
        json_true.contains("true"),
        "C2: applied=true must serialise a `true` boolean, got {:?}", json_true
    );
    assert!(
        !json_true.contains("false"),
        "C2: applied=true must not serialise `false`, got {:?}", json_true
    );
    let back_true = mend::parse_json(&json_true).expect("C2: applied=true JSON must parse");
    assert_eq!(
        back_true.applied, true,
        "C2: applied=true must round-trip to true, got {:?}", back_true.applied
    );

    // applied = false: JSON has the key, a `false` literal, no `true`, round-trips false.
    let json_false = mend::emit_json(&make(false));
    assert!(
        json_false.contains("applied"),
        "C2: JSON must include an `applied` key, got {:?}", json_false
    );
    assert!(
        json_false.contains("false"),
        "C2: applied=false must serialise a `false` boolean, got {:?}", json_false
    );
    assert!(
        !json_false.contains("true"),
        "C2: applied=false must not serialise `true`, got {:?}", json_false
    );
    let back_false = mend::parse_json(&json_false).expect("C2: applied=false JSON must parse");
    assert_eq!(
        back_false.applied, false,
        "C2: applied=false must round-trip to false, got {:?}", back_false.applied
    );

    println!("C2_OK");
}
"##;
    let p = run_probe("c2", src);
    assert!(
        p.ran_ok && p.output.contains("C2_OK"),
        "emit_json must include an `applied` boolean reflecting the input flag; probe output:\n{}",
        p.output
    );
}

/// C3 — the emitted JSON parses back into the same structure. The probe builds a
/// result with both edit shapes and multi-line text, serialises it, parses it back,
/// and asserts the reconstructed struct equals the original both as a whole and
/// field by field (path, original, proposed, edits, applied). It then re-serialises
/// the parsed struct and asserts the JSON is byte-identical — the schema is stable
/// under a round trip. A mutant that mangles, drops, or reorders any field breaks
/// the equality and the probe fails.
#[test]
fn c3_round_trips_to_same_structure() {
    let src = r##"
fn main() {
    let result = mend::JsonResult {
        path: "src/main.rs".to_string(),
        original: "first\nsecond\n".to_string(),
        proposed: "first\nCHANGED\n".to_string(),
        edits: vec![
            mend::ParsedEdit::SearchReplace {
                search: "second".to_string(),
                replace: "CHANGED".to_string(),
            },
            mend::ParsedEdit::WholeFile { body: "first\nCHANGED\n".to_string() },
        ],
        applied: true,
    };

    let json = mend::emit_json(&result);
    let parsed = mend::parse_json(&json).expect("C3: emitted JSON must parse back");

    // The round trip reconstructs the exact same structure, whole and field by field.
    assert_eq!(parsed, result, "C3: round-tripped struct must equal the original, got {:?}", parsed);
    assert_eq!(parsed.path, result.path, "C3: path must survive the round trip");
    assert_eq!(parsed.original, result.original, "C3: original must survive the round trip");
    assert_eq!(parsed.proposed, result.proposed, "C3: proposed must survive the round trip");
    assert_eq!(parsed.edits, result.edits, "C3: edits must survive the round trip");
    assert_eq!(parsed.applied, result.applied, "C3: applied must survive the round trip");

    // Re-serialising the parsed struct reproduces the identical JSON: the schema is stable.
    assert_eq!(
        mend::emit_json(&parsed), json,
        "C3: re-serialising the parsed struct must reproduce the JSON"
    );

    println!("C3_OK");
}
"##;
    let p = run_probe("c3", src);
    assert!(
        p.ran_ok && p.output.contains("C3_OK"),
        "emitted JSON must parse back into the same structure; probe output:\n{}",
        p.output
    );
}
