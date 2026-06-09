//! A deterministic, network-free [`Completer`] stand-in for tests.
//!
//! [`MockCompleter`] replays a pre-scripted list of responses in first-in,
//! first-out order — one response consumed per [`complete`](Completer::complete)
//! call — and records every prompt it receives. Exhausting the script is a
//! deterministic [`MendError::Completer`], never a panic, so identical scripts
//! and call sequences yield identical behaviour on every run. This is the seam
//! that makes end-to-end tests possible without ever touching a network.
//!
//! The [`Completer`] contract hands the mock only `&self`, yet replaying must
//! advance a cursor and recording must grow a log; both mutations live behind
//! [`RefCell`]s so the shared-reference method can still update them.

use std::cell::RefCell;
use std::collections::VecDeque;

use crate::{Completer, MendError, Result};

/// A [`Completer`] that replays scripted responses and records received prompts.
///
/// Construct it with [`MockCompleter::new`], passing the responses to replay in
/// order. Each [`complete`](Completer::complete) call records its prompt and
/// returns the next scripted response; once the script is empty, further calls
/// return [`MendError::Completer`]. The recorded prompts are available via
/// [`MockCompleter::prompts`] for assertions.
pub struct MockCompleter {
    /// Remaining scripted responses, consumed from the front (FIFO).
    responses: RefCell<VecDeque<String>>,
    /// Every prompt received, in the order it arrived.
    prompts: RefCell<Vec<String>>,
}

impl MockCompleter {
    /// Build a mock that will replay `responses` in FIFO order.
    pub fn new(responses: Vec<String>) -> Self {
        MockCompleter {
            responses: RefCell::new(VecDeque::from(responses)),
            prompts: RefCell::new(Vec::new()),
        }
    }

    /// The prompts received so far, in the order they were received.
    pub fn prompts(&self) -> Vec<String> {
        self.prompts.borrow().clone()
    }

    /// Record `prompt` and replay the next scripted response.
    ///
    /// This is the inherent twin of the [`Completer::complete`] implementation,
    /// callable on a `MockCompleter` value without the trait in scope; the trait
    /// impl delegates here so both paths share one behaviour.
    pub fn complete(&self, prompt: &str) -> Result<String> {
        self.prompts.borrow_mut().push(prompt.to_string());
        match self.responses.borrow_mut().pop_front() {
            Some(response) => Ok(response),
            None => Err(MendError::Completer(
                "mock completer script exhausted: no scripted response remaining".to_string(),
            )),
        }
    }
}

impl Completer for MockCompleter {
    fn complete(&self, prompt: &str) -> Result<String> {
        MockCompleter::complete(self, prompt)
    }
}
