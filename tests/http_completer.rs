//! Frozen acceptance tests for T-08.02 — Implement HTTP Completer.
//!
//! The criteria are about a Rust type whose behaviour is an actual HTTP exchange:
//! an `HttpCompleter` that implements `Completer` by issuing one HTTP POST of the
//! prompt to the endpoint from `Config`, returning the response body as the
//! completion, and mapping any non-2xx status to `MendError::Completer`. Exactly
//! as the T-00.02 error, T-00.03 CLI, T-01.01 trait, and T-01.02 mock tests do,
//! the honest way to observe this is to *compile and run a tiny probe program
//! against the real `mend` crate*. The probes pin the contract the criteria
//! imply; this test crate stays std-only and never references the
//! not-yet-existing API directly, so it always compiles. Redness comes from each
//! probe failing to *build* (the `HttpCompleter` type does not exist yet), not
//! from this file failing to parse.
//!
//! Unlike the other completer tasks, the *point* of this one is the HTTP call, so
//! each probe stands up a one-shot HTTP server on the loopback interface
//! (`127.0.0.1:0`, an OS-assigned port) using only `std::net`. The probe points
//! the `Config.endpoint` at that server's URL, drives `complete`, and then
//! inspects the exact bytes the server received and controls the exact status the
//! server returns. This keeps the test fully deterministic and confined to
//! loopback — it never reaches an external network — while observing the three
//! real criteria: that a POST carrying the prompt arrives at the configured
//! endpoint, that the configured model and endpoint are the ones used, and that a
//! non-2xx status becomes `MendError::Completer`. Each probe is built into an
//! isolated `CARGO_TARGET_DIR` and run `--offline`, so it neither contends with
//! the outer build lock nor fetches crates; the GREEN implementation therefore
//! must speak HTTP with std-only raw TCP, with no third-party client crate.
//!
//! The probes pin the minimal, idiomatic contract the criteria imply:
//!   - `mend::HttpCompleter::new(&Config) -> HttpCompleter`   (model + endpoint from Config)
//!   - `<HttpCompleter as Completer>::complete(&self, &str) -> mend::Result<String>`
//!   - one HTTP `POST` per call, carrying the prompt, to `Config.endpoint`
//!   - a 2xx response yields `Ok(body)`; any non-2xx yields `MendError::Completer`
//!
//!   C1: an `HttpCompleter` implements `Completer` by POSTing the prompt to the
//!       configured endpoint (and returns the response body as the completion).
//!   C2: it reads `model` and `endpoint` from `Config`.
//!   C3: a non-2xx HTTP response yields `MendError::Completer`.

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
    dir.push(format!("mend-httpcompleter-{tag}-{}-{nanos}", std::process::id()));
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
/// `mend` crate via a path dependency. A standalone `[workspace]` keeps the probe
/// from inheriting any ambient workspace; an isolated target dir keeps it off the
/// outer build lock; `--offline` keeps it off the crate network.
///
/// In the red state `mend` has no `HttpCompleter`, so the probe fails to resolve
/// and `ran_ok` is false. Once the `mend` library defines the type the probe
/// references, it builds, runs its loopback HTTP exchange, and prints its success
/// sentinel.
fn run_probe(tag: &str, main_src: &str) -> Probe {
    let crate_dir = unique_temp_dir(tag);
    let target_dir = unique_temp_dir(&format!("{tag}-target"));

    // Literal (single-quoted) TOML string so backslashes in the path are not
    // treated as escapes on any platform.
    let manifest = format!(
        "[package]\n\
         name = \"mend_httpcompleter_probe\"\n\
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

/// A self-contained, std-only one-shot HTTP server prepended to every probe.
///
/// `start_server(status, reason, body)` binds a loopback socket on an OS-assigned
/// port, returns the `http://127.0.0.1:PORT` URL plus a channel that yields the
/// raw request bytes once a connection has been served, and — on the single
/// connection it accepts — replies with the given status line and body. This is
/// *test-side* scaffolding: its own operators are never mutated (the mutation
/// gate only flips operators inside the `mend` crate), so it can be as elaborate
/// as it needs to be to read the request faithfully.
const SERVER_HELPER: &str = r#"
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// Stand up a one-shot loopback HTTP server. Returns its URL and a receiver that
/// yields the full request the (single) client sent.
fn start_server(status: u16, reason: &str, resp_body: &str) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let addr = listener.local_addr().expect("local addr");
    let url = format!("http://{}", addr);
    let (tx, rx) = mpsc::channel();
    let reason = reason.to_string();
    let resp_body = resp_body.to_string();
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            // Never block the test forever if the client speaks malformed HTTP.
            let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
            let request = read_http_request(&mut stream);
            let response = format!(
                "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                status,
                reason,
                resp_body.len(),
                resp_body
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
            let _ = tx.send(request);
        }
    });
    (url, rx)
}

/// Read a full HTTP request: headers terminated by CRLFCRLF, then exactly the
/// `Content-Length` body bytes the headers advertise.
fn read_http_request(stream: &mut TcpStream) -> String {
    let mut buf: Vec<u8> = Vec::new();
    let mut tmp = [0u8; 1024];
    let header_end = loop {
        if let Some(pos) = find_subsequence(&buf, b"\r\n\r\n") {
            break pos + 4;
        }
        match stream.read(&mut tmp) {
            Ok(0) => break buf.len(),
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
            Err(_) => break buf.len(),
        }
    };
    let headers = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let want = content_length(&headers);
    while buf.len() < header_end + want {
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
            Err(_) => break,
        }
    }
    String::from_utf8_lossy(&buf).to_string()
}

fn find_subsequence(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

fn content_length(headers: &str) -> usize {
    for line in headers.lines() {
        let lower = line.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("content-length:") {
            if let Ok(n) = rest.trim().parse::<usize>() {
                return n;
            }
        }
    }
    0
}
"#;

/// Assemble a probe's `main.rs` from the shared server helper plus a test body.
fn probe_src(body: &str) -> String {
    format!("{SERVER_HELPER}\n{body}")
}

/// C1 — an `HttpCompleter` implements `Completer` by POSTing the prompt to the
/// configured endpoint, and hands back the response body as the completion.
///
/// The probe constructs the completer behind a `Box<dyn Completer>` (proving the
/// trait is implemented and object-safe), drives it against a loopback server
/// that returns 200 with a known body, then asserts three things at once: the
/// request the server saw began with `POST ` (the verb), it carried the prompt in
/// its bytes (the prompt was sent), and `complete` returned exactly the server's
/// response body (the completion is the model's reply). A GET, a dropped prompt,
/// or a garbled body each break one of these.
#[test]
fn c1_implements_completer_by_posting_prompt_and_returns_body() {
    let body = r#"
use mend::Completer;

fn main() {
    let (url, rx) = start_server(200, "OK", "THE-COMPLETION-TEXT");

    let cfg = mend::Config {
        model: String::from("model-c1"),
        endpoint: url,
        token_budget: 1,
    };

    // Behind a trait object: this only compiles if HttpCompleter: Completer and
    // the trait is object-safe.
    let completer: Box<dyn Completer> = Box::new(mend::HttpCompleter::new(&cfg));
    let got = completer
        .complete("PROMPT-NEEDLE-C1")
        .expect("a 2xx response must yield Ok");

    let request = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("the server should have received exactly one request");

    assert!(
        request.starts_with("POST "),
        "the request must be an HTTP POST; request was:\n{}",
        request
    );
    assert!(
        request.contains("PROMPT-NEEDLE-C1"),
        "the POST must carry the prompt; request was:\n{}",
        request
    );
    assert_eq!(
        got, "THE-COMPLETION-TEXT",
        "complete must return the response body as the completion"
    );

    println!("C1_OK");
}
"#;
    let p = run_probe("c1", &probe_src(body));
    assert!(
        p.ran_ok && p.output.contains("C1_OK"),
        "HttpCompleter must implement Completer and POST the prompt to the endpoint, returning the body; probe output:\n{}",
        p.output
    );
}

/// C2 — the completer reads `model` and `endpoint` from `Config`.
///
/// The endpoint is proven *structurally*: the server is bound to a fresh,
/// OS-assigned loopback port, so the only way a request can arrive at it is if the
/// completer dialled the URL it was handed in `Config.endpoint` — a hard-coded
/// endpoint could not reach this port. The model is proven by content: a
/// distinctive model string is placed in `Config.model` and the probe asserts
/// that exact string appears in the bytes the server received. Both must hold, so
/// dropping either field from the request fails the probe.
#[test]
fn c2_reads_model_and_endpoint_from_config() {
    let body = r#"
use mend::Completer;

fn main() {
    let (url, rx) = start_server(200, "OK", "ok");

    let cfg = mend::Config {
        model: String::from("DISTINCTIVE-MODEL-C2"),
        endpoint: url,
        token_budget: 7,
    };

    let completer = mend::HttpCompleter::new(&cfg);
    let _ = completer
        .complete("hello-c2")
        .expect("a 2xx response must yield Ok");

    // Receiving anything at all proves the endpoint came from Config: the port is
    // random, so a hard-coded URL could never reach this server.
    let request = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("the configured endpoint must have been dialled (request received)");

    assert!(
        request.contains("DISTINCTIVE-MODEL-C2"),
        "the model from Config must be carried in the request; request was:\n{}",
        request
    );

    println!("C2_OK");
}
"#;
    let p = run_probe("c2", &probe_src(body));
    assert!(
        p.ran_ok && p.output.contains("C2_OK"),
        "HttpCompleter must read both model and endpoint from Config; probe output:\n{}",
        p.output
    );
}

/// C3 (lower boundary, success) — a 200 response is `Ok`, not an error.
///
/// This pins the inclusive *lower* edge of the 2xx success band: at exactly 200
/// the call must succeed and return the body. It kills any mutant that narrows
/// the lower bound past 200 (e.g. `>= 200` flipped to `> 200`).
#[test]
fn c3_status_200_lower_boundary_is_success() {
    let body = r#"
use mend::Completer;

fn main() {
    let (url, _rx) = start_server(200, "OK", "body-200");

    let cfg = mend::Config {
        model: String::from("m"),
        endpoint: url,
        token_budget: 1,
    };

    let completer = mend::HttpCompleter::new(&cfg);
    let got = completer.complete("p");
    match got {
        Ok(text) => assert_eq!(text, "body-200", "200 must return the response body"),
        other => panic!("status 200 must be Ok, got {:?}", other),
    }

    println!("C3_OK");
}
"#;
    let p = run_probe("c3lo", &probe_src(body));
    assert!(
        p.ran_ok && p.output.contains("C3_OK"),
        "status 200 (lower 2xx boundary) must be a success; probe output:\n{}",
        p.output
    );
}

/// C3 (upper boundary, success) — a 299 response is `Ok`, not an error.
///
/// This pins the inclusive *upper* edge of the 2xx success band: at 299 the call
/// must still succeed. It kills any mutant that widens or shifts the upper bound
/// (e.g. `< 300` flipped to `> 300`, which would reject 299).
#[test]
fn c3_status_299_upper_boundary_is_success() {
    let body = r#"
use mend::Completer;

fn main() {
    let (url, _rx) = start_server(299, "Almost", "body-299");

    let cfg = mend::Config {
        model: String::from("m"),
        endpoint: url,
        token_budget: 1,
    };

    let completer = mend::HttpCompleter::new(&cfg);
    let got = completer.complete("p");
    match got {
        Ok(text) => assert_eq!(text, "body-299", "299 must return the response body"),
        other => panic!("status 299 must be Ok, got {:?}", other),
    }

    println!("C3_OK");
}
"#;
    let p = run_probe("c3hi", &probe_src(body));
    assert!(
        p.ran_ok && p.output.contains("C3_OK"),
        "status 299 (upper 2xx boundary) must be a success; probe output:\n{}",
        p.output
    );
}

/// C3 (just past 2xx) — a 300 response yields `MendError::Completer`.
///
/// This pins the first status *outside* the success band: 300 must be an error,
/// and specifically the `Completer` variant (not `Io`/`Parse`). It kills any
/// mutant that makes the upper bound inclusive (e.g. `< 300` flipped to
/// `<= 300`), which would wrongly accept 300.
#[test]
fn c3_status_300_just_past_2xx_is_completer_error() {
    let body = r#"
use mend::Completer;

fn main() {
    let (url, _rx) = start_server(300, "Multiple Choices", "nope");

    let cfg = mend::Config {
        model: String::from("m"),
        endpoint: url,
        token_budget: 1,
    };

    let completer = mend::HttpCompleter::new(&cfg);
    match completer.complete("p") {
        Err(mend::MendError::Completer(_)) => {}
        other => panic!("status 300 must be MendError::Completer, got {:?}", other),
    }

    println!("C3_OK");
}
"#;
    let p = run_probe("c3_300", &probe_src(body));
    assert!(
        p.ran_ok && p.output.contains("C3_OK"),
        "status 300 (just past 2xx) must be MendError::Completer; probe output:\n{}",
        p.output
    );
}

/// C3 (just below 2xx) — a 199 response yields `MendError::Completer`.
///
/// This pins the status just *below* the success band: 199 is non-2xx and must be
/// an error. It kills any mutant that loosens the lower bound the wrong way (e.g.
/// `>= 200` flipped to `<= 200`), which would wrongly accept 199.
#[test]
fn c3_status_199_just_below_2xx_is_completer_error() {
    let body = r#"
use mend::Completer;

fn main() {
    let (url, _rx) = start_server(199, "Early", "nope");

    let cfg = mend::Config {
        model: String::from("m"),
        endpoint: url,
        token_budget: 1,
    };

    let completer = mend::HttpCompleter::new(&cfg);
    match completer.complete("p") {
        Err(mend::MendError::Completer(_)) => {}
        other => panic!("status 199 must be MendError::Completer, got {:?}", other),
    }

    println!("C3_OK");
}
"#;
    let p = run_probe("c3_199", &probe_src(body));
    assert!(
        p.ran_ok && p.output.contains("C3_OK"),
        "status 199 (just below 2xx) must be MendError::Completer; probe output:\n{}",
        p.output
    );
}

/// C3 (client error) — a 404 response yields `MendError::Completer`.
///
/// A representative 4xx far from the boundary: it must be a `Completer` error. In
/// combination with the success cases it kills the `&&`→`||` mutant in a
/// `status >= 200 && status < 300` test, which would otherwise treat every status
/// as success.
#[test]
fn c3_status_404_is_completer_error() {
    let body = r#"
use mend::Completer;

fn main() {
    let (url, _rx) = start_server(404, "Not Found", "missing");

    let cfg = mend::Config {
        model: String::from("m"),
        endpoint: url,
        token_budget: 1,
    };

    let completer = mend::HttpCompleter::new(&cfg);
    match completer.complete("p") {
        Err(mend::MendError::Completer(_)) => {}
        other => panic!("status 404 must be MendError::Completer, got {:?}", other),
    }

    println!("C3_OK");
}
"#;
    let p = run_probe("c3_404", &probe_src(body));
    assert!(
        p.ran_ok && p.output.contains("C3_OK"),
        "status 404 must be MendError::Completer; probe output:\n{}",
        p.output
    );
}

/// C3 (server error) — a 500 response yields `MendError::Completer`.
///
/// A representative 5xx: it must be a `Completer` error too, confirming the rule
/// is "any non-2xx", not just a single hard-coded code.
#[test]
fn c3_status_500_is_completer_error() {
    let body = r#"
use mend::Completer;

fn main() {
    let (url, _rx) = start_server(500, "Internal Server Error", "boom");

    let cfg = mend::Config {
        model: String::from("m"),
        endpoint: url,
        token_budget: 1,
    };

    let completer = mend::HttpCompleter::new(&cfg);
    match completer.complete("p") {
        Err(mend::MendError::Completer(_)) => {}
        other => panic!("status 500 must be MendError::Completer, got {:?}", other),
    }

    println!("C3_OK");
}
"#;
    let p = run_probe("c3_500", &probe_src(body));
    assert!(
        p.ran_ok && p.output.contains("C3_OK"),
        "status 500 must be MendError::Completer; probe output:\n{}",
        p.output
    );
}
