//! Frozen acceptance tests for T-04.02 — Parse fenced code blocks.
//!
//! The criteria are about a Rust API (a function that turns an LLM response
//! string into a `Vec<ParsedEdit>` of whole-file replacements), so — exactly as
//! the T-02.02 imports and T-04.01 parsed-edit tests do — the honest way to
//! observe them is to *compile and run a tiny probe program against the real
//! `mend` crate*. The probes pin the contract the criteria imply; this test crate
//! stays std-only and never references the not-yet-existing API directly, so it
//! always compiles. Redness comes from each probe failing to *build* (the
//! `parse_fenced` function does not exist yet), not from this file failing to
//! parse. Each probe is built into an isolated `CARGO_TARGET_DIR` and run
//! `--offline`, so it neither contends with the outer build lock nor touches the
//! network.
//!
//! The probes pin the minimal, idiomatic contract the criteria imply:
//!   - `mend::parse_fenced(response: &str) -> mend::Result<Vec<mend::ParsedEdit>>`
//!   - a single ```` ``` ```` fenced block becomes exactly one
//!     `ParsedEdit::WholeFile { body }` whose `body` is the verbatim text between
//!     the line after the opening fence and the closing fence (the closing
//!     newline is part of that verbatim slice).
//!   - the info string / language tag on the opening fence line is NOT part of
//!     the body; when the opening fence carries no tag, no content line is
//!     consumed as one.
//!   - an opening fence with no matching closing fence is `MendError::Parse`,
//!     while a properly closed fence is `Ok`.
//!   - N fenced blocks yield N edits, one per block, in document order.
//!
//!   C1: extracts the body of a triple-backtick fenced block as a whole-file `ParsedEdit`.
//!   C2: the language tag after the opening fence is stripped from the body.
//!   C3: input with an unterminated fence yields `MendError::Parse`.
//!   C4: multiple fenced blocks return one ParsedEdit per block in document order.

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
    dir.push(format!("mend-fenced-{tag}-{}-{nanos}", std::process::id()));
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
/// In the red state `mend` has no `parse_fenced` function, so the probe fails to
/// resolve and `ran_ok` is false. Once the `mend` library exposes the function
/// the probe references, it builds, runs, and prints its success sentinel. A
/// probe that panics (e.g. on a broken invariant) exits non-zero and prints no
/// sentinel, so a panic fails the corresponding test.
fn run_probe(tag: &str, main_src: &str) -> Probe {
    let crate_dir = unique_temp_dir(tag);
    let target_dir = unique_temp_dir(&format!("{tag}-target"));

    // Literal (single-quoted) TOML string so backslashes in the path are not
    // treated as escapes on any platform.
    let manifest = format!(
        "[package]\n\
         name = \"mend_fenced_probe\"\n\
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

/// C1 — a single triple-backtick fenced block is extracted as exactly one
/// whole-file `ParsedEdit`. The probe feeds a response holding one fenced block
/// and asserts three things: the result is `Ok`; the vector holds exactly one
/// edit (one block → one edit, not zero and not many); that edit is the
/// `WholeFile` variant carrying the verbatim block body. The body is the text
/// between the opening fence line and the closing fence, including the newline
/// that precedes the closing fence, and excluding the fence lines themselves.
#[test]
fn c1_single_fence_is_whole_file_edit() {
    let src = r##"
fn main() {
    let response = "```rust\nfn main() {}\n```";
    let edits = mend::parse_fenced(response).expect("C1: a closed fence must parse Ok");

    assert_eq!(edits.len(), 1, "C1: one fenced block must yield exactly one edit");
    assert_eq!(
        edits[0],
        mend::ParsedEdit::WholeFile { body: "fn main() {}\n".to_string() },
        "C1: the edit must be a WholeFile carrying the verbatim fenced body"
    );

    println!("C1_OK");
}
"##;
    let p = run_probe("c1", src);
    assert!(
        p.ran_ok && p.output.contains("C1_OK"),
        "parse_fenced must extract a fenced block's body as one whole-file edit; probe output:\n{}",
        p.output
    );
}

/// C2 — the language tag on the opening fence line is stripped from the body,
/// and ONLY the tag is stripped. The probe checks both sides of the "is there a
/// tag" boundary so an implementation cannot pass by always (or never) dropping
/// the first inner line:
///   - with a `rust` tag: the body equals the inner lines exactly, the word
///     `rust` never appears, and the true first content line survives.
///   - with no tag at all: the body is identical, proving the first content line
///     was NOT mistaken for a tag and dropped.
#[test]
fn c2_language_tag_stripped_from_body() {
    let src = r##"
fn main() {
    let tagged = "```rust\nuse std::fmt;\nfn main() {}\n```";
    let edits = mend::parse_fenced(tagged).expect("C2: a closed fence must parse Ok");
    assert_eq!(edits.len(), 1, "C2: one block must yield one edit");
    let body = match &edits[0] {
        mend::ParsedEdit::WholeFile { body } => body.clone(),
        other => panic!("C2: expected a WholeFile edit, got {:?}", other),
    };
    assert_eq!(
        body, "use std::fmt;\nfn main() {}\n",
        "C2: the body must be the inner lines verbatim, with the tag line removed"
    );
    assert!(!body.contains("rust"), "C2: the language tag must not appear in the body, got {:?}", body);

    // No-tag side: the first content line must NOT be swallowed as a tag.
    let untagged = "```\nuse std::fmt;\nfn main() {}\n```";
    let edits2 = mend::parse_fenced(untagged).expect("C2: a closed untagged fence must parse Ok");
    assert_eq!(edits2.len(), 1, "C2: one untagged block must yield one edit");
    let body2 = match &edits2[0] {
        mend::ParsedEdit::WholeFile { body } => body.clone(),
        other => panic!("C2: expected a WholeFile edit, got {:?}", other),
    };
    assert_eq!(
        body2, "use std::fmt;\nfn main() {}\n",
        "C2: with no tag, every content line including the first must be preserved"
    );

    println!("C2_OK");
}
"##;
    let p = run_probe("c2", src);
    assert!(
        p.ran_ok && p.output.contains("C2_OK"),
        "parse_fenced must strip the language tag but keep all content lines; probe output:\n{}",
        p.output
    );
}

/// C3 — an opening fence with no matching closing fence is a `MendError::Parse`,
/// while a properly closed fence is `Ok`. The probe pins both sides of the
/// "is the fence terminated" boundary: the unterminated input must be exactly the
/// `Parse` variant (not `Ok`, not some other error variant), and the otherwise
/// identical but closed input must be `Ok`. Asserting both sides kills a mutant
/// that always errors or never errors.
#[test]
fn c3_unterminated_fence_is_parse_error() {
    let src = r##"
fn main() {
    let unterminated = "```rust\nfn main() {}\n";
    match mend::parse_fenced(unterminated) {
        Err(mend::MendError::Parse(_)) => {}
        other => panic!("C3: an unterminated fence must be MendError::Parse, got {:?}", other),
    }

    // The closed counterpart must succeed — the error is about termination only.
    let terminated = "```rust\nfn main() {}\n```";
    match mend::parse_fenced(terminated) {
        Ok(edits) => assert_eq!(edits.len(), 1, "C3: a closed fence must parse to one edit"),
        other => panic!("C3: a closed fence must be Ok, got {:?}", other),
    }

    println!("C3_OK");
}
"##;
    let p = run_probe("c3", src);
    assert!(
        p.ran_ok && p.output.contains("C3_OK"),
        "parse_fenced must reject an unterminated fence with MendError::Parse; probe output:\n{}",
        p.output
    );
}

/// C4 — multiple fenced blocks return one `ParsedEdit` per block, in document
/// order. The probe feeds a response with two distinct fenced blocks and asserts
/// the vector length matches the block count and that each element is exactly the
/// block at that position — `edits[0]` is the first block's body and `edits[1]`
/// is the second's — pinning "one per block" and the order together. Swapping the
/// two assertions would fail, so the order is genuinely constrained.
#[test]
fn c4_multiple_blocks_in_document_order() {
    let src = r##"
fn main() {
    let response = "```rust\nAAA\n```\n```python\nBBB\n```";
    let edits = mend::parse_fenced(response).expect("C4: two closed fences must parse Ok");

    assert_eq!(edits.len(), 2, "C4: two blocks must yield exactly two edits");
    assert_eq!(
        edits[0],
        mend::ParsedEdit::WholeFile { body: "AAA\n".to_string() },
        "C4: the first edit must be the first block, in order"
    );
    assert_eq!(
        edits[1],
        mend::ParsedEdit::WholeFile { body: "BBB\n".to_string() },
        "C4: the second edit must be the second block, in order"
    );

    println!("C4_OK");
}
"##;
    let p = run_probe("c4", src);
    assert!(
        p.ran_ok && p.output.contains("C4_OK"),
        "parse_fenced must return one edit per block in document order; probe output:\n{}",
        p.output
    );
}
