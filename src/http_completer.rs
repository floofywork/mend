//! The real backend behind the [`Completer`] trait: a std-only HTTP client.
//!
//! [`HttpCompleter`] issues exactly one HTTP `POST` per [`complete`](Completer::complete)
//! call, carrying the prompt to the `endpoint` taken from [`Config`] and naming
//! the configured `model` in the request body. A `2xx` response yields its body
//! as the completion; any non-`2xx` status collapses into
//! [`MendError::Completer`]. The exchange is spoken over a raw [`TcpStream`] with
//! no third-party HTTP client, so the crate stays dependency-light and the type
//! never leaks above the trait that hides it.
//!
//! Streaming and retry/backoff are deliberately out of scope: one request, one
//! response, one result.

use std::io::{Read, Write};
use std::net::TcpStream;

use crate::{Completer, Config, MendError, Result};

/// A [`Completer`] that talks to a real completion endpoint over HTTP.
///
/// Construct it with [`HttpCompleter::new`], handing it the [`Config`] whose
/// `model` and `endpoint` fields drive every request.
pub struct HttpCompleter {
    /// The model identifier sent with each request.
    model: String,
    /// The endpoint URL each request is POSTed to.
    endpoint: String,
}

impl HttpCompleter {
    /// Build a completer that reads its `model` and `endpoint` from `config`.
    pub fn new(config: &Config) -> Self {
        HttpCompleter {
            model: config.model.clone(),
            endpoint: config.endpoint.clone(),
        }
    }
}

impl Completer for HttpCompleter {
    fn complete(&self, prompt: &str) -> Result<String> {
        // The configured endpoint is an `http://host:port` URL; the address we
        // dial is everything after the scheme.
        let address = self
            .endpoint
            .strip_prefix("http://")
            .unwrap_or(&self.endpoint);

        let mut stream = TcpStream::connect(address)
            .map_err(|e| MendError::Completer(format!("failed to connect to {address}: {e}")))?;

        // One POST carrying the prompt and the configured model.
        let body = format!(
            "{{\"model\":{:?},\"prompt\":{:?}}}",
            self.model, prompt
        );
        let request = format!(
            "POST / HTTP/1.1\r\n\
             Host: {address}\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\
             \r\n\
             {body}",
            body.len()
        );
        stream
            .write_all(request.as_bytes())
            .map_err(|e| MendError::Completer(format!("failed to send request: {e}")))?;

        // The server replies with `Connection: close`, so reading to EOF yields
        // the whole response.
        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .map_err(|e| MendError::Completer(format!("failed to read response: {e}")))?;

        // Split the response into its head and body on the blank line, then read
        // the numeric status code out of the status line.
        let mut halves = response.splitn(2, "\r\n\r\n");
        let head = halves.next().unwrap_or("");
        let resp_body = halves.next().unwrap_or("").to_string();
        let status: u16 = head
            .lines()
            .next()
            .unwrap_or("")
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse().ok())
            .unwrap_or(0);

        if status >= 200 && status < 300 {
            Ok(resp_body)
        } else {
            Err(MendError::Completer(format!(
                "endpoint returned non-2xx status {status}"
            )))
        }
    }
}
