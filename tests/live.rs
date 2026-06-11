//! Manually-run live integration test for `HttpCompleter` (T-08.03).
//!
//! This test is fenced behind the opt-in `live` Cargo feature, so the default
//! `cargo test` run never compiles it and therefore never reaches the network.
//! Run it explicitly against a real endpoint with:
//!
//!     cargo test --features live -- live_http_completer_against_real_endpoint
//!
//! It builds an `HttpCompleter` from the ambient `Config` — whose `endpoint` and
//! `model` fields name a real backend — and drives one real completion. The
//! `expect` is the pass/fail: a connection failure or any non-2xx status surfaces
//! as `MendError::Completer` and fails the test.

#[cfg(feature = "live")]
#[test]
fn live_http_completer_against_real_endpoint() {
    use mend::{Completer, Config, HttpCompleter};

    let config = Config::load().expect("load config for the live run");
    let completer = HttpCompleter::new(&config);
    let _completion = completer
        .complete("ping")
        .expect("live endpoint must return a completion");
}
