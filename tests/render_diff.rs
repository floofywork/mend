//! Frozen acceptance tests for T-06.02 — Render coloured diff.
//!
//! The criteria are about a Rust API (a pure function that turns a unified diff
//! string plus a TTY signal into a coloured-or-plain diff string), so — exactly as
//! the T-02.01 structure, T-02.03 budget, T-04.04 parse-unified-diff, T-05.04 apply
//! and T-06.01 compute-unified-diff tests do — the honest way to observe them is to
//! *compile and run a tiny probe program against the real `mend` crate*. The probes
//! pin the contract the criteria imply; this test crate stays std-only and never
//! references the not-yet-existing API directly, so it always compiles. Redness
//! comes from each probe failing to *build* (the `render_diff` function does not
//! exist yet), not from this file failing to parse. Each probe is built into an
//! isolated `CARGO_TARGET_DIR` and run `--offline`, so it neither contends with the
//! outer build lock nor touches the network.
//!
//! The probes pin the minimal, idiomatic contract the criteria imply:
//!   - `mend::render_diff(diff: &str, color: bool) -> String`
//!     — a pure, in-memory rendering. It consumes a unified diff string (the
//!     T-06.01 output: an `@@ … @@` header line followed by body lines, each
//!     prefixed by a marker — a space for context, `-` for removed, `+` for added)
//!     and a boolean TTY signal (`true` when stdout is a terminal). It returns the
//!     human-facing diff: per-line ANSI styling when `color` is true, the verbatim
//!     diff otherwise. It does no diff computation (out of scope, consumes T-06.01)
//!     and no paging or width wrapping (out of scope).
//!   - when `color` is true, added (`+`) lines render green, removed (`-`) lines
//!     red, and `@@` hunk headers cyan, via ANSI SGR escapes (green `\x1b[32m`, red
//!     `\x1b[31m`, cyan `\x1b[36m`); context lines stay uncoloured.
//!   - when `color` is false (stdout is not a TTY), every colour code is omitted and
//!     the output equals the input diff verbatim.
//!   - an empty diff renders the empty string, in either mode.
//!   - the cross-cutting invariant: stripping the ANSI codes from the coloured
//!     output recovers the original diff text exactly (colour adds styling, never
//!     content).
//!
//!   C1: added lines render green, removed lines red, and hunk headers cyan via ANSI codes.
//!   C2: when stdout is not a TTY, all colour codes are omitted.
//!   C3: an empty diff renders an empty string.

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
        "mend-render-diff-{tag}-{}-{nanos}",
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
/// In the red state `mend` has no `render_diff` function, so the probe fails to
/// resolve and `ran_ok` is false. Once the `mend` library exposes the function the
/// probe references, it builds, runs, and prints its success sentinel. A probe that
/// panics (e.g. on a broken invariant) exits non-zero and prints no sentinel, so a
/// panic fails the corresponding test.
fn run_probe(tag: &str, main_src: &str) -> Probe {
    let crate_dir = unique_temp_dir(tag);
    let target_dir = unique_temp_dir(&format!("{tag}-target"));

    // Literal (single-quoted) TOML string so backslashes in the path are not
    // treated as escapes on any platform.
    let manifest = format!(
        "[package]\n\
         name = \"mend_render_diff_probe\"\n\
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

/// C1 — with a TTY signal, `render_diff` styles each line by its marker: added
/// (`+`) lines green, removed (`-`) lines red, and `@@` hunk headers cyan, via ANSI
/// SGR escapes; context lines stay uncoloured. The probe feeds a hand-built unified
/// diff carrying one of each line type and asserts the colour is tied to the
/// correct marker (`\x1b[36m@@`, `\x1b[31m-removed`, `\x1b[32m+added`) and that the
/// colours are NOT swapped (the added line is never red, the removed line never
/// green) — so a mutant that swaps two colours, or colours the wrong line type,
/// dies. The context line stays verbatim (` context` with no escape before it). The
/// invariant — stripping every ANSI code recovers the original diff text exactly —
/// is the tight cross-check: it proves colour adds only styling, never content, and
/// kills a mutant that drops, reorders, or mangles a line.
#[test]
fn c1_colours_added_removed_and_header() {
    let src = r##"
fn main() {
    let diff = "@@ -1,3 +1,3 @@\n context\n-removed\n+added\n";
    let coloured = mend::render_diff(diff, true);

    // A real diff with colour enabled renders non-empty.
    assert!(!coloured.is_empty(), "C1: coloured diff must be non-empty, got {:?}", coloured);

    // Each colour is tied to its line type via the standard ANSI SGR escapes.
    assert!(
        coloured.contains("\x1b[36m@@"),
        "C1: the hunk header must render cyan, got {:?}",
        coloured
    );
    assert!(
        coloured.contains("\x1b[31m-removed"),
        "C1: a removed line must render red, got {:?}",
        coloured
    );
    assert!(
        coloured.contains("\x1b[32m+added"),
        "C1: an added line must render green, got {:?}",
        coloured
    );

    // Colours are not swapped: added is never red, removed is never green.
    assert!(
        !coloured.contains("\x1b[31m+added"),
        "C1: an added line must not render red, got {:?}",
        coloured
    );
    assert!(
        !coloured.contains("\x1b[32m-removed"),
        "C1: a removed line must not render green, got {:?}",
        coloured
    );

    // The context line carries no colour: it appears verbatim between newlines,
    // with no ANSI escape introduced before it.
    assert!(
        coloured.contains("\n context\n"),
        "C1: a context line must stay uncoloured, got {:?}",
        coloured
    );

    // Invariant: stripping the ANSI codes recovers the original diff text exactly.
    assert_eq!(
        strip_ansi(&coloured),
        diff,
        "C1: stripping ANSI codes must recover the original diff, got {:?}",
        coloured
    );

    println!("C1_OK");
}

// Remove every ANSI CSI escape (`\x1b[ … m`) and return the remaining text.
fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            for d in chars.by_ref() {
                if d == 'm' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}
"##;
    let p = run_probe("c1", src);
    assert!(
        p.ran_ok && p.output.contains("C1_OK"),
        "render_diff must colour added lines green, removed red, and headers cyan; probe output:\n{}",
        p.output
    );
}

/// C2 — when stdout is not a TTY (`color = false`), every colour code is omitted and
/// the output equals the input diff verbatim. The probe pins BOTH sides of the TTY
/// boundary on the same non-empty diff: with the flag false the output equals the
/// diff exactly and contains no ESC byte, while with the flag true it does carry
/// ANSI escapes and differs from the plain rendering. Asserting both sides kills a
/// mutant that ignores the flag (always colours, or never colours) and one that
/// inverts it.
#[test]
fn c2_non_tty_omits_all_colour() {
    let src = r##"
fn main() {
    let diff = "@@ -1,3 +1,3 @@\n context\n-removed\n+added\n";

    // Not a TTY: output is the diff verbatim, with no ANSI escape at all.
    let plain = mend::render_diff(diff, false);
    assert_eq!(
        plain, diff,
        "C2: non-TTY output must equal the original diff verbatim, got {:?}",
        plain
    );
    assert!(
        !plain.contains('\x1b'),
        "C2: non-TTY output must contain no ANSI escape, got {:?}",
        plain
    );

    // Boundary: the SAME diff WITH a TTY does carry colour — the flag truly toggles.
    let coloured = mend::render_diff(diff, true);
    assert!(
        coloured.contains('\x1b'),
        "C2: TTY output must contain ANSI escapes, got {:?}",
        coloured
    );
    assert_ne!(
        coloured, plain,
        "C2: TTY and non-TTY rendering of a non-empty diff must differ, got {:?}",
        coloured
    );

    println!("C2_OK");
}
"##;
    let p = run_probe("c2", src);
    assert!(
        p.ran_ok && p.output.contains("C2_OK"),
        "render_diff must omit all colour codes when stdout is not a TTY; probe output:\n{}",
        p.output
    );
}

/// C3 — an empty diff renders the empty string, in either mode. The probe pins both
/// the TTY and non-TTY paths returning exactly `""` for empty input, and — as the
/// boundary — a non-empty diff rendering non-empty in both modes. Asserting both
/// sides kills a mutant that always returns empty (it would fail the non-empty case)
/// and one that emits stray bytes for empty input (it would fail the empty case).
#[test]
fn c3_empty_diff_renders_empty() {
    let src = r##"
fn main() {
    // An empty diff renders empty, regardless of the TTY signal.
    assert_eq!(
        mend::render_diff("", true),
        "",
        "C3: an empty diff must render the empty string with a TTY"
    );
    assert_eq!(
        mend::render_diff("", false),
        "",
        "C3: an empty diff must render the empty string without a TTY"
    );

    // Boundary: a non-empty diff renders non-empty in both modes.
    let diff = "@@ -1,3 +1,3 @@\n context\n-removed\n+added\n";
    assert!(
        !mend::render_diff(diff, true).is_empty(),
        "C3: a non-empty diff must render non-empty with a TTY"
    );
    assert!(
        !mend::render_diff(diff, false).is_empty(),
        "C3: a non-empty diff must render non-empty without a TTY"
    );

    println!("C3_OK");
}
"##;
    let p = run_probe("c3", src);
    assert!(
        p.ran_ok && p.output.contains("C3_OK"),
        "render_diff must render an empty diff as the empty string; probe output:\n{}",
        p.output
    );
}
