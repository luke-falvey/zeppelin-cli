//! Printing responses.

use serde_json::Value;

#[derive(Clone, Copy)]
pub struct Output {
    /// Print the full `{"status", "message", "body"}` envelope instead of just `body`.
    pub raw: bool,
    /// Single-line JSON.
    pub compact: bool,
}

impl Output {
    pub fn json(&self, v: &Value) {
        let s = if self.compact {
            serde_json::to_string(v)
        } else {
            serde_json::to_string_pretty(v)
        };
        println!("{}", s.expect("serialisable JSON"));
    }

    /// Print an API response. By default only `body` is printed; a string body
    /// (e.g. a new note id) is printed bare so it can be captured with `$(...)`.
    /// Responses without a body print nothing.
    pub fn response(&self, envelope: &Value) {
        if self.raw {
            return self.json(envelope);
        }
        match envelope.get("body") {
            None | Some(Value::Null) => {}
            Some(Value::String(s)) => println!("{s}"),
            Some(b) => self.json(b),
        }
    }
}
