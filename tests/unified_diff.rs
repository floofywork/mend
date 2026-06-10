//! Frozen acceptance tests for T-04.04 — Parse unified diff.
//!
//! The criteria are about a Rust API (a function that turns an LLM response
//! string into a `Vec<ParsedEdit>` of targeted edits recovered from a single-file
//! unified diff), so — exactly as the T-04.01 parsed-edit, T-04.02 fenced and
//! T-04.03 search/replace tests do — the honest way to observe them is to *compile
//! and run a tiny probe program against the real `mend` crate*. The probes pin the
//! contract the criteria imply; this test crate stays std-only and never
//! references the not-yet-existing API directly, so it always compiles. Redness
//! comes from each probe failing to *build* (the `parse_unified_diff` function
//! does not exist yet), not from this file failing to parse. Each probe is built
//! into an isolated `CARGO_TARGET_DIR` and run `--offline`, so it neither contends
//! with the outer build lock nor touches the network.
//!
//! The probes pin the minimal, idiomatic contract the criteria imply:
//!   - `mend::parse_unified_diff(response: &str)
//!         -> mend::Result<Vec<mend::ParsedEdit>>`
//!   - a single `@@ … @@` hunk becomes exactly one
//!     `ParsedEdit::SearchReplace { search, replace }`. Its lines are classified by
//!     their leading marker, which is stripped from the stored content:
//!       * a leading-space line is context — it goes to BOTH the search and the
//!         replace side;
//!       * a `-` line is removed — it goes to the search side only;
//!       * a `+` line is added — it goes to the replace side only.
//!     The search side thus reconstructs the pre-image (context + removed) and the
//!     replace side the post-image (context + added), in document order, each
//!     content line carrying its trailing newline.
//!   - a hunk header opens with `@@` and is delimited by ` @@`; a header that
//!     opens with `@@` but is not so delimited fails to parse and is
//!     `MendError::Parse`, while a well-formed header is `Ok`.
//!
//!   C1: parses an `@@ … @@` hunk of `+`/`-`/space lines into a ParsedEdit.
//!   C2: context (leading-space) and removed (`-`) lines populate the search side
//!       of the edit (and added `+` lines do not).
//!   C3: a hunk header that fails to parse yields `MendError::Parse`.

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
        "mend-unified-diff-{tag}-{}-{nanos}",
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
/// In the red state `mend` has no `parse_unified_diff` function, so the probe
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
         name = \"mend_unified_diff_probe\"\n\
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

/// C1 — a single `@@ … @@` hunk made of `+`/`-`/space lines is parsed into exactly
/// one targeted `ParsedEdit`. The probe feeds one hunk that exercises all three
/// line kinds — a context (leading-space) line, a removed (`-`) line and an added
/// (`+`) line — and asserts: the result is `Ok`; the vector holds exactly one edit
/// (one hunk → one edit, not zero and not many); that edit is the `SearchReplace`
/// variant; and both halves are exactly the marker-stripped content lines, with
/// the search side reconstructing the pre-image (context + removed) and the
/// replace side the post-image (context + added). Because the two halves carry
/// different text, an implementation that swapped them, leaked a marker character
/// into a half, or misrouted a line kind would fail the exact-equality assertion.
#[test]
fn c1_single_hunk_is_one_parsed_edit() {
    let src = r##"
fn main() {
    let response = "@@ -1,2 +1,2 @@\n keep\n-old\n+new\n";
    let edits = mend::parse_unified_diff(response)
        .expect("C1: a well-formed hunk must parse Ok");

    assert_eq!(edits.len(), 1, "C1: one hunk must yield exactly one edit");
    assert_eq!(
        edits[0],
        mend::ParsedEdit::SearchReplace {
            search: "keep\nold\n".to_string(),
            replace: "keep\nnew\n".to_string(),
        },
        "C1: one hunk becomes one SearchReplace edit derived solely from its +/-/space lines"
    );

    println!("C1_OK");
}
"##;
    let p = run_probe("c1", src);
    assert!(
        p.ran_ok && p.output.contains("C1_OK"),
        "parse_unified_diff must parse one hunk into one (search, replace) edit; probe output:\n{}",
        p.output
    );
}

/// C2 — context (leading-space) and removed (`-`) lines populate the search side
/// of the edit, and added (`+`) lines do not. The probe pins both sides of the
/// "which side does each line kind feed" boundary using a hunk with one of each
/// kind, all carrying distinct text:
///   - the search side must equal the context line followed by the removed line,
///     in order (`"ctx\nrem\n"`), and must NOT contain the added line's text —
///     killing a mutant that drops context or removed from the search side, or
///     that leaks the added line into it;
///   - symmetrically, the replace side must equal the context line followed by the
///     added line (`"ctx\nadd\n"`) and must NOT contain the removed line's text —
///     so a mutant that routes every line to both sides, or swaps `-` and `+`,
///     is caught on the opposite side too.
#[test]
fn c2_context_and_removed_populate_search_side() {
    let src = r##"
fn main() {
    let response = "@@ -1,2 +1,2 @@\n ctx\n-rem\n+add\n";
    let edits = mend::parse_unified_diff(response)
        .expect("C2: a well-formed hunk must parse Ok");
    assert_eq!(edits.len(), 1, "C2: one hunk must yield one edit");

    let (search, replace) = match &edits[0] {
        mend::ParsedEdit::SearchReplace { search, replace } => (search.clone(), replace.clone()),
        other => panic!("C2: expected a SearchReplace edit, got {:?}", other),
    };

    // Context + removed lines, in order, populate the search side...
    assert_eq!(
        search, "ctx\nrem\n",
        "C2: context (space) and removed (-) lines populate the search side, in order"
    );
    // ...and the added (+) line must NOT leak onto the search side.
    assert!(
        !search.contains("add"),
        "C2: added (+) lines must not appear on the search side, got {:?}",
        search
    );

    // Both-sides boundary: the replace side carries context + added, never the
    // removed line — so misrouting either marker is caught here too.
    assert_eq!(
        replace, "ctx\nadd\n",
        "C2: context (space) and added (+) lines populate the replace side, in order"
    );
    assert!(
        !replace.contains("rem"),
        "C2: removed (-) lines must not appear on the replace side, got {:?}",
        replace
    );

    println!("C2_OK");
}
"##;
    let p = run_probe("c2", src);
    assert!(
        p.ran_ok && p.output.contains("C2_OK"),
        "parse_unified_diff must route context+removed lines to the search side and added lines to the replace side; probe output:\n{}",
        p.output
    );
}

/// C3 — a hunk header that fails to parse is a `MendError::Parse`, while an
/// otherwise-identical well-formed header is `Ok`. The probe pins both sides of
/// the "is the header well-formed" boundary: the malformed header opens with `@@`
/// but is not delimited by the closing ` @@`, and must be exactly the `Parse`
/// variant (not `Ok`, not some other error variant); the counterpart that differs
/// only by adding the closing ` @@` must be `Ok` with one edit. Asserting both
/// sides kills a mutant that always errors or never errors.
#[test]
fn c3_malformed_header_is_parse_error() {
    let src = r##"
fn main() {
    // Opens with `@@` but is not closed with the ` @@` delimiter — malformed.
    let malformed = "@@ -1,2 +1,2\n keep\n-old\n+new\n";
    match mend::parse_unified_diff(malformed) {
        Err(mend::MendError::Parse(_)) => {}
        other => panic!("C3: a malformed hunk header must be MendError::Parse, got {:?}", other),
    }

    // The well-formed counterpart (closing ` @@` present) must succeed — the error
    // is about the malformed header only.
    let well_formed = "@@ -1,2 +1,2 @@\n keep\n-old\n+new\n";
    match mend::parse_unified_diff(well_formed) {
        Ok(edits) => assert_eq!(edits.len(), 1, "C3: a well-formed hunk must parse to one edit"),
        other => panic!("C3: a well-formed hunk header must be Ok, got {:?}", other),
    }

    println!("C3_OK");
}
"##;
    let p = run_probe("c3", src);
    assert!(
        p.ran_ok && p.output.contains("C3_OK"),
        "parse_unified_diff must reject a malformed hunk header with MendError::Parse and accept a well-formed one; probe output:\n{}",
        p.output
    );
}
