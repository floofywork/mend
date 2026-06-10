//! Frozen acceptance tests for T-04.03 — Parse search-replace blocks.
//!
//! The criteria are about a Rust API (a function that turns an LLM response
//! string into a `Vec<ParsedEdit>` of targeted search/replace edits), so —
//! exactly as the T-04.01 parsed-edit and T-04.02 fenced tests do — the honest
//! way to observe them is to *compile and run a tiny probe program against the
//! real `mend` crate*. The probes pin the contract the criteria imply; this test
//! crate stays std-only and never references the not-yet-existing API directly,
//! so it always compiles. Redness comes from each probe failing to *build* (the
//! `parse_search_replace` function does not exist yet), not from this file
//! failing to parse. Each probe is built into an isolated `CARGO_TARGET_DIR` and
//! run `--offline`, so it neither contends with the outer build lock nor touches
//! the network.
//!
//! The probes pin the minimal, idiomatic contract the criteria imply:
//!   - `mend::parse_search_replace(response: &str)
//!         -> mend::Result<Vec<mend::ParsedEdit>>`
//!   - a single `<<<<<<< SEARCH … ======= … >>>>>>> REPLACE` block becomes
//!     exactly one `ParsedEdit::SearchReplace { search, replace }`, where the
//!     `search` half is the verbatim lines between the `<<<<<<< SEARCH` marker and
//!     the `=======` divider, and the `replace` half is the verbatim lines between
//!     the divider and the `>>>>>>> REPLACE` marker. Each content line carries its
//!     trailing newline; the three marker lines are excluded.
//!   - N blocks yield N edits, one per block, in document order.
//!   - a block missing its `=======` divider is `MendError::Parse`, while a
//!     well-formed block with the divider is `Ok`.
//!
//!   C1: parses a `<<<<<<< SEARCH … ======= … >>>>>>> REPLACE` block into a (search, replace) ParsedEdit.
//!   C2: multiple blocks yield multiple ParsedEdits in document order.
//!   C3: a block missing the `=======` divider yields `MendError::Parse`.

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
        "mend-search-replace-{tag}-{}-{nanos}",
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
/// In the red state `mend` has no `parse_search_replace` function, so the probe
/// fails to resolve and `ran_ok` is false. Once the `mend` library exposes the
/// function the probe references, it builds, runs, and prints its success
/// sentinel. A probe that panics (e.g. on a broken invariant) exits non-zero and
/// prints no sentinel, so a panic fails the corresponding test.
fn run_probe(tag: &str, main_src: &str) -> Probe {
    let crate_dir = unique_temp_dir(tag);
    let target_dir = unique_temp_dir(&format!("{tag}-target"));

    // Literal (single-quoted) TOML string so backslashes in the path are not
    // treated as escapes on any platform.
    let manifest = format!(
        "[package]\n\
         name = \"mend_search_replace_probe\"\n\
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

/// C1 — a single `<<<<<<< SEARCH … ======= … >>>>>>> REPLACE` block is parsed
/// into exactly one `ParsedEdit::SearchReplace`. The probe feeds one block whose
/// search and replace halves are distinct and multi-line, then asserts: the
/// result is `Ok`; the vector holds exactly one edit (one block → one edit, not
/// zero and not many); that edit is the `SearchReplace` variant; and both halves
/// are the verbatim content lines between the markers, split exactly on the
/// `=======` divider. Because the two halves carry different text, an
/// implementation that swapped them, leaked a marker line into a half, or merged
/// them would fail the exact-equality assertion.
#[test]
fn c1_single_block_is_search_replace_edit() {
    let src = r##"
fn main() {
    let response = "<<<<<<< SEARCH\nlet x = 1;\nlet y = 2;\n=======\nlet x = 10;\nlet y = 20;\n>>>>>>> REPLACE";
    let edits = mend::parse_search_replace(response)
        .expect("C1: a well-formed search/replace block must parse Ok");

    assert_eq!(edits.len(), 1, "C1: one block must yield exactly one edit");
    assert_eq!(
        edits[0],
        mend::ParsedEdit::SearchReplace {
            search: "let x = 1;\nlet y = 2;\n".to_string(),
            replace: "let x = 10;\nlet y = 20;\n".to_string(),
        },
        "C1: the edit must be a SearchReplace whose halves are split exactly on the divider"
    );

    println!("C1_OK");
}
"##;
    let p = run_probe("c1", src);
    assert!(
        p.ran_ok && p.output.contains("C1_OK"),
        "parse_search_replace must parse one block into one (search, replace) edit; probe output:\n{}",
        p.output
    );
}

/// C2 — multiple blocks yield one `ParsedEdit` per block, in document order. The
/// probe feeds a response with two distinct blocks and asserts the vector length
/// matches the block count and that each element is exactly the block at that
/// position — `edits[0]` is the first block and `edits[1]` is the second —
/// pinning "one per block" and the order together. The two blocks carry distinct
/// text, so swapping the two assertions would fail and the order is genuinely
/// constrained.
#[test]
fn c2_multiple_blocks_in_document_order() {
    let src = r##"
fn main() {
    let response = "<<<<<<< SEARCH\nAAA\n=======\nBBB\n>>>>>>> REPLACE\n<<<<<<< SEARCH\nCCC\n=======\nDDD\n>>>>>>> REPLACE";
    let edits = mend::parse_search_replace(response)
        .expect("C2: two well-formed blocks must parse Ok");

    assert_eq!(edits.len(), 2, "C2: two blocks must yield exactly two edits");
    assert_eq!(
        edits[0],
        mend::ParsedEdit::SearchReplace {
            search: "AAA\n".to_string(),
            replace: "BBB\n".to_string(),
        },
        "C2: the first edit must be the first block, in order"
    );
    assert_eq!(
        edits[1],
        mend::ParsedEdit::SearchReplace {
            search: "CCC\n".to_string(),
            replace: "DDD\n".to_string(),
        },
        "C2: the second edit must be the second block, in order"
    );

    println!("C2_OK");
}
"##;
    let p = run_probe("c2", src);
    assert!(
        p.ran_ok && p.output.contains("C2_OK"),
        "parse_search_replace must return one edit per block in document order; probe output:\n{}",
        p.output
    );
}

/// C3 — a block missing its `=======` divider is a `MendError::Parse`, while an
/// otherwise-identical block that includes the divider is `Ok`. The probe pins
/// both sides of the "is the block well-formed" boundary: the divider-less input
/// must be exactly the `Parse` variant (not `Ok`, not some other error variant),
/// and the input that adds the divider must be `Ok` with one edit. Asserting both
/// sides kills a mutant that always errors or never errors.
#[test]
fn c3_missing_divider_is_parse_error() {
    let src = r##"
fn main() {
    let missing = "<<<<<<< SEARCH\nAAA\nBBB\n>>>>>>> REPLACE";
    match mend::parse_search_replace(missing) {
        Err(mend::MendError::Parse(_)) => {}
        other => panic!("C3: a block missing the ======= divider must be MendError::Parse, got {:?}", other),
    }

    // The counterpart that adds the divider must succeed — the error is about the
    // missing divider only.
    let present = "<<<<<<< SEARCH\nAAA\n=======\nBBB\n>>>>>>> REPLACE";
    match mend::parse_search_replace(present) {
        Ok(edits) => assert_eq!(edits.len(), 1, "C3: a block with the divider must parse to one edit"),
        other => panic!("C3: a block with the divider must be Ok, got {:?}", other),
    }

    println!("C3_OK");
}
"##;
    let p = run_probe("c3", src);
    assert!(
        p.ran_ok && p.output.contains("C3_OK"),
        "parse_search_replace must reject a block missing its ======= divider with MendError::Parse; probe output:\n{}",
        p.output
    );
}
