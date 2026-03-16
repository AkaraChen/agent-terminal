use serde::{Deserialize, Serialize};

/// Requests sent by IPC clients to the session server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    /// Write raw bytes into the PTY master (simulating user input).
    WriteInput { data: String },
    /// Ask the session for its current output buffer.
    GetOutput,
}

/// Response returned by the session server over the Unix socket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Ok,
    Output {
        /// Raw bytes (base64-encoded) of the captured output.
        raw_b64: String,
        /// Rendered screen text (current VT100 state).
        screen: String,
    },
    Error {
        message: String,
    },
}
