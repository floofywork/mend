//! Frozen acceptance tests for T-04.05 — Detect response format.
//!
//! The criteria are about a Rust API: a *dispatcher* that takes a raw LLM
//! response string, auto-detects which of the three known formats it is in, and
//! routes it to the matching single-purpose parser, returning that parser's
//! `Vec<ParsedEdit>`. As with the T-04.02 fenced, T-04.03 search/replace, and
//! T-04.04 unified-diff tests, the honest way to observe the contract is to
//! *compile and run a tiny probe program against the real `mend` crate*. The
//! probes pin the contract the criteria imply; this test crate stays std-only and
//! never references the not-yet-existing API directly, so it always compiles.
//! Redness comes from each probe failing to *build* (the `parse_response`
//! function does not exist yet), not from this file failing to parse. Each probe
//! is built into an isolated `CARGO_TARGET_DIR` and run `--offline`, so it neither
//! contends with the outer build lock nor touches the network.
//!
//! The probes pin the minimal, idiomatic contract the criteria imply:
//!   - `mend::parse_response(response: &str) -> mend::Result<Vec<mend::ParsedEdit>>`
//!   - a response containing the `<<<<<<< SEARCH` marker is routed to the
//!     search/replace parser, so the result is exactly what
//!     `parse_search_replace` would return for that response.
//!   - a response containing a `@@ ` hunk marker (but no `<<<<<<< SEARCH`) is
//!     routed to the unified-diff parser, so the result is exactly what
//!     `parse_unified_diff` would return.
//!   - a response with only a fenced block (no `<<<<<<< SEARCH`, no `@@ `) is
//!     routed to the fenced parser, yielding `WholeFile` edits.
//!   - a response matching none of the three known formats is `MendError::Parse`.
//!   - selection follows a fixed first-match precedence
//!     (search/replace before unified-diff before fenced), so a response carrying
//!     more than one format's markers always routes the same way.
//!
//!   C1: a response containing `<<<<<<< SEARCH` routes to the search-replace parser.
//!   C2: a response containing `@@ ` hunk markers routes to the unified-diff parser.
//!   C3: a response with only a fenced block routes to the fenced parser.
//!   C4: a response matching no known format yields `MendError::Parse`.

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
    dir.push(format!("mend-dispatch-{tag}-{}-{nanos}", std::process::id()));
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
/// In the red state `mend` has no `parse_response` function, so the probe fails
/// to resolve and `ran_ok` is false. Once the `mend` library exposes the function
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
         name = \"mend_dispatch_probe\"\n\
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

/// C1 — a response carrying the `<<<<<<< SEARCH` marker routes to the
/// search/replace parser. The probe pins both that the result is the
/// search/replace parser's output (a `SearchReplace` edit with the verbatim
/// halves) AND the fixed precedence: the same response also contains a `@@ `
/// substring, yet it must NOT be parsed as a unified diff. If the dispatcher
/// skipped the `<<<<<<< SEARCH` check (or checked `@@ ` first), the result would
/// differ from `parse_search_replace`'s, so the exact-equality assertion fails.
#[test]
fn c1_search_replace_marker_routes_to_search_replace() {
    let src = r##"
fn main() {
    // Contains a `@@ ` substring as well, to pin that search/replace wins.
    let response = "<<<<<<< SEARCH\n@@ a @@\n=======\nbar\n>>>>>>> REPLACE";

    let edits = mend::parse_response(response)
        .expect("C1: a response with a SEARCH marker must parse Ok via the search/replace parser");

    let expected = mend::parse_search_replace(response)
        .expect("C1: the search/replace parser itself must accept this response");
    assert_eq!(
        edits, expected,
        "C1: a `<<<<<<< SEARCH` response must route to the search/replace parser"
    );
    assert_eq!(
        edits,
        vec![mend::ParsedEdit::SearchReplace {
            search: "@@ a @@\n".to_string(),
            replace: "bar\n".to_string(),
        }],
        "C1: the routed result must be the search/replace edit, not a unified-diff parse"
    );

    println!("C1_OK");
}
"##;
    let p = run_probe("c1", src);
    assert!(
        p.ran_ok && p.output.contains("C1_OK"),
        "parse_response must route a `<<<<<<< SEARCH` response to the search/replace parser; probe output:\n{}",
        p.output
    );
}

/// C2 — a response carrying a `@@ ` hunk marker (and NOT `<<<<<<< SEARCH`) routes
/// to the unified-diff parser. The probe pins both that the result equals
/// `parse_unified_diff`'s output AND the precedence over the fenced parser: the
/// same response is wrapped in a ```` ``` ```` fence, yet it must be parsed as a
/// diff (a `SearchReplace` reconstructing the pre/post images), not as a
/// whole-file fenced block. A dispatcher that checked the fence first would yield
/// a `WholeFile` edit and fail the equality assertion.
#[test]
fn c2_hunk_marker_routes_to_unified_diff() {
    let src = r##"
fn main() {
    // Wrapped in a fence, to pin that the `@@ ` diff route wins over fenced.
    let response = "```diff\n@@ -1 +1 @@\n-old\n+new\n```";

    let edits = mend::parse_response(response)
        .expect("C2: a response with a `@@ ` marker must parse Ok via the unified-diff parser");

    let expected = mend::parse_unified_diff(response)
        .expect("C2: the unified-diff parser itself must accept this response");
    assert_eq!(
        edits, expected,
        "C2: a `@@ ` response must route to the unified-diff parser"
    );
    assert_eq!(
        edits,
        vec![mend::ParsedEdit::SearchReplace {
            search: "old\n".to_string(),
            replace: "new\n".to_string(),
        }],
        "C2: the routed result must be the unified-diff parse, not a whole-file fenced block"
    );

    println!("C2_OK");
}
"##;
    let p = run_probe("c2", src);
    assert!(
        p.ran_ok && p.output.contains("C2_OK"),
        "parse_response must route a `@@ ` response to the unified-diff parser; probe output:\n{}",
        p.output
    );
}

/// C3 — a response with only a fenced block (no `<<<<<<< SEARCH`, no `@@ `)
/// routes to the fenced parser. The probe asserts the result equals
/// `parse_fenced`'s output and that the edit is the `WholeFile` variant — a shape
/// only the fenced parser produces — so a dispatcher that mis-routed this to
/// either of the other two parsers (which emit `SearchReplace`) would fail.
#[test]
fn c3_fenced_only_routes_to_fenced() {
    let src = r##"
fn main() {
    let response = "```rust\nfn main() {}\n```";

    let edits = mend::parse_response(response)
        .expect("C3: a fenced-only response must parse Ok via the fenced parser");

    let expected = mend::parse_fenced(response)
        .expect("C3: the fenced parser itself must accept this response");
    assert_eq!(
        edits, expected,
        "C3: a fenced-only response must route to the fenced parser"
    );
    assert_eq!(
        edits,
        vec![mend::ParsedEdit::WholeFile { body: "fn main() {}\n".to_string() }],
        "C3: the routed result must be the fenced parser's whole-file edit"
    );

    println!("C3_OK");
}
"##;
    let p = run_probe("c3", src);
    assert!(
        p.ran_ok && p.output.contains("C3_OK"),
        "parse_response must route a fenced-only response to the fenced parser; probe output:\n{}",
        p.output
    );
}

/// C4 — a response matching none of the three known formats is
/// `MendError::Parse`. The probe pins both sides of the "is this a known format"
/// boundary: prose carrying none of the three markers must be exactly the `Parse`
/// variant (not `Ok`, not some other error variant), while a recognizable
/// response is `Ok` — proving the error is about non-recognition, not a
/// dispatcher that always errors. Asserting both sides kills a mutant that always
/// returns `Ok` or always errors.
#[test]
fn c4_unknown_format_is_parse_error() {
    let src = r##"
fn main() {
    let unknown = "I'm sorry, I cannot help with that request.";
    match mend::parse_response(unknown) {
        Err(mend::MendError::Parse(_)) => {}
        other => panic!("C4: an unrecognized response must be MendError::Parse, got {:?}", other),
    }

    // A recognizable response must still be Ok — the error is non-recognition only.
    let known = "```rust\nfn main() {}\n```";
    match mend::parse_response(known) {
        Ok(edits) => assert_eq!(edits.len(), 1, "C4: a recognized response must parse to its edits"),
        other => panic!("C4: a recognized response must be Ok, got {:?}", other),
    }

    println!("C4_OK");
}
"##;
    let p = run_probe("c4", src);
    assert!(
        p.ran_ok && p.output.contains("C4_OK"),
        "parse_response must yield MendError::Parse for an unrecognized response; probe output:\n{}",
        p.output
    );
}
