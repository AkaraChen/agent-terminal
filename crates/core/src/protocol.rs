use serde::{Deserialize, Serialize};

/// Requests sent by IPC clients to the session server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    /// Write raw bytes into the PTY master (simulating user input).
    WriteInput { data: String },
    /// Ask the session for its current output buffer.
    GetOutput,
    /// Subscribe to output stream.
    Subscribe,
    /// Unsubscribe from output stream.
    Unsubscribe,
    /// Authenticate with the server (TCP mode).
    Authenticate { token: String },
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
    /// Streaming output chunk (for subscribers).
    OutputChunk {
        /// Raw bytes (base64-encoded) of the new output chunk.
        raw_b64: String,
    },
    Error {
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Request serialization ────────────────────────────────────────────

    #[test]
    fn test_write_input_serializes_type_tag() {
        let req = Request::WriteInput { data: "ls\n".to_string() };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"type\":\"write_input\""), "json = {json}");
    }

    #[test]
    fn test_write_input_serializes_data() {
        let req = Request::WriteInput { data: "echo hi\n".to_string() };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"data\":\"echo hi\\n\""), "json = {json}");
    }

    #[test]
    fn test_get_output_serializes_type_tag() {
        let req = Request::GetOutput;
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"type\":\"get_output\""), "json = {json}");
    }

    // ── Request deserialization ──────────────────────────────────────────

    #[test]
    fn test_write_input_deserializes() {
        let json = r#"{"type":"write_input","data":"echo hello\n"}"#;
        let req: Request = serde_json::from_str(json).unwrap();
        match req {
            Request::WriteInput { data } => assert_eq!(data, "echo hello\n"),
            _ => panic!("expected WriteInput"),
        }
    }

    #[test]
    fn test_get_output_deserializes() {
        let json = r#"{"type":"get_output"}"#;
        let req: Request = serde_json::from_str(json).unwrap();
        assert!(matches!(req, Request::GetOutput));
    }

    // ── Request clone + debug ────────────────────────────────────────────

    #[test]
    fn test_request_clone_write_input() {
        let req = Request::WriteInput { data: "test".to_string() };
        let cloned = req.clone();
        match cloned {
            Request::WriteInput { data } => assert_eq!(data, "test"),
            _ => panic!("expected WriteInput"),
        }
    }

    #[test]
    fn test_request_debug_is_not_empty() {
        let s = format!("{:?}", Request::GetOutput);
        assert!(!s.is_empty());
    }

    // ── Response serialization roundtrip ────────────────────────────────

    #[test]
    fn test_response_ok_roundtrip() {
        let resp = Response::Ok;
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"type\":\"ok\""), "json = {json}");
        let back: Response = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, Response::Ok));
    }

    #[test]
    fn test_response_output_roundtrip() {
        let resp = Response::Output {
            raw_b64: "aGVsbG8=".to_string(),
            screen: "hello".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: Response = serde_json::from_str(&json).unwrap();
        match back {
            Response::Output { raw_b64, screen } => {
                assert_eq!(raw_b64, "aGVsbG8=");
                assert_eq!(screen, "hello");
            }
            _ => panic!("expected Output, got something else"),
        }
    }

    #[test]
    fn test_response_error_roundtrip() {
        let resp = Response::Error { message: "session not found".to_string() };
        let json = serde_json::to_string(&resp).unwrap();
        let back: Response = serde_json::from_str(&json).unwrap();
        match back {
            Response::Error { message } => assert_eq!(message, "session not found"),
            _ => panic!("expected Error"),
        }
    }

    #[test]
    fn test_response_clone() {
        let resp = Response::Error { message: "oops".to_string() };
        let cloned = resp.clone();
        match cloned {
            Response::Error { message } => assert_eq!(message, "oops"),
            _ => panic!("expected Error"),
        }
    }

    #[test]
    fn test_response_error_type_tag() {
        let resp = Response::Error { message: "err".to_string() };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"type\":\"error\""), "json = {json}");
    }

    #[test]
    fn test_response_output_type_tag() {
        let resp = Response::Output { raw_b64: String::new(), screen: String::new() };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"type\":\"output\""), "json = {json}");
    }

    // ── Subscribe/Unsubscribe ───────────────────────────────────────────

    #[test]
    fn test_subscribe_deserializes() {
        let json = r#"{"type":"subscribe"}"#;
        let req: Request = serde_json::from_str(json).unwrap();
        assert!(matches!(req, Request::Subscribe));
    }

    #[test]
    fn test_unsubscribe_deserializes() {
        let json = r#"{"type":"unsubscribe"}"#;
        let req: Request = serde_json::from_str(json).unwrap();
        assert!(matches!(req, Request::Unsubscribe));
    }

    #[test]
    fn test_subscribe_serializes_type_tag() {
        let req = Request::Subscribe;
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"type\":\"subscribe\""), "json = {json}");
    }

    #[test]
    fn test_unsubscribe_serializes_type_tag() {
        let req = Request::Unsubscribe;
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"type\":\"unsubscribe\""), "json = {json}");
    }

    #[test]
    fn test_output_chunk_roundtrip() {
        let resp = Response::OutputChunk {
            raw_b64: "aGVsbG8=".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: Response = serde_json::from_str(&json).unwrap();
        match back {
            Response::OutputChunk { raw_b64 } => {
                assert_eq!(raw_b64, "aGVsbG8=");
            }
            _ => panic!("expected OutputChunk"),
        }
    }

    #[test]
    fn test_output_chunk_type_tag() {
        let resp = Response::OutputChunk { raw_b64: "dGVzdA==".to_string() };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"type\":\"output_chunk\""), "json = {json}");
    }

    // ── Authenticate ───────────────────────────────────────────────────

    #[test]
    fn test_authenticate_serializes_type_tag() {
        let req = Request::Authenticate { token: "secret123".to_string() };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"type\":\"authenticate\""), "json = {json}");
    }

    #[test]
    fn test_authenticate_serializes_token() {
        let req = Request::Authenticate { token: "my_token".to_string() };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"token\":\"my_token\""), "json = {json}");
    }

    #[test]
    fn test_authenticate_deserializes() {
        let json = r#"{"type":"authenticate","token":"test_token"}"#;
        let req: Request = serde_json::from_str(json).unwrap();
        match req {
            Request::Authenticate { token } => assert_eq!(token, "test_token"),
            _ => panic!("expected Authenticate"),
        }
    }
}
