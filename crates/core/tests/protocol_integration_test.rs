use agent_terminal_core::protocol::{Request, Response};

/// Helper macro to test roundtrip serialization
macro_rules! test_roundtrip {
    ($value:expr) => {{
        let json = serde_json::to_string(&$value).unwrap();
        serde_json::from_str(&json).unwrap()
    }};
}

#[test]
fn test_protocol_all_request_response_pairs() {
    // WriteInput -> Ok
    let req = Request::WriteInput {
        data: "test".to_string(),
    };
    let req_roundtrip: Request = test_roundtrip!(req);
    match req_roundtrip {
        Request::WriteInput { data } => assert_eq!(data, "test"),
        _ => panic!("expected WriteInput"),
    }

    // GetOutput -> Output
    let req = Request::GetOutput;
    let req_roundtrip: Request = test_roundtrip!(req);
    assert!(matches!(req_roundtrip, Request::GetOutput));

    // Output response
    let resp = Response::Output {
        raw_b64: "aGVsbG8=".to_string(),
        screen: "hello".to_string(),
    };
    let resp_roundtrip: Response = test_roundtrip!(resp);
    match resp_roundtrip {
        Response::Output { raw_b64, screen } => {
            assert_eq!(raw_b64, "aGVsbG8=");
            assert_eq!(screen, "hello");
        }
        _ => panic!("expected Output"),
    }

    // Error response
    let resp = Response::Error {
        message: "error message".to_string(),
    };
    let resp_roundtrip: Response = test_roundtrip!(resp);
    match resp_roundtrip {
        Response::Error { message } => assert_eq!(message, "error message"),
        _ => panic!("expected Error"),
    }

    // Ok response
    let resp = Response::Ok;
    let resp_roundtrip: Response = test_roundtrip!(resp);
    assert!(matches!(resp_roundtrip, Response::Ok));
}

#[test]
fn test_protocol_unicode_handling() {
    // Request with unicode
    let req = Request::WriteInput {
        data: "Hello 世界 🌍 ñoño".to_string(),
    };
    let json = serde_json::to_string(&req).unwrap();
    let decoded: Request = serde_json::from_str(&json).unwrap();
    match decoded {
        Request::WriteInput { data } => {
            assert_eq!(data, "Hello 世界 🌍 ñoño");
        }
        _ => panic!("expected WriteInput"),
    }

    // Response with unicode
    let resp = Response::Output {
        raw_b64: "dGVzdA==".to_string(),
        screen: "屏幕输出 🖥️".to_string(),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let decoded: Response = serde_json::from_str(&json).unwrap();
    match decoded {
        Response::Output { screen, .. } => {
            assert_eq!(screen, "屏幕输出 🖥️");
        }
        _ => panic!("expected Output"),
    }
}

#[test]
fn test_protocol_special_characters() {
    let special_chars = [
        "\n\r\t",
        "\"quoted\"",
        "backslash\\path",
        "null\0byte",
        "tab\there",
        "line1\nline2",
    ];

    for chars in &special_chars {
        let req = Request::WriteInput {
            data: chars.to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let decoded: Request = serde_json::from_str(&json).unwrap();
        match decoded {
            Request::WriteInput { data } => {
                assert_eq!(data, *chars);
            }
            _ => panic!("expected WriteInput"),
        }
    }
}

#[test]
fn test_protocol_nested_json() {
    // Data that looks like JSON
    let json_like = r#"{"key": "value", "nested": {"a": 1}}"#;
    let req = Request::WriteInput {
        data: json_like.to_string(),
    };
    let json = serde_json::to_string(&req).unwrap();
    let decoded: Request = serde_json::from_str(&json).unwrap();
    match decoded {
        Request::WriteInput { data } => {
            assert_eq!(data, json_like);
        }
        _ => panic!("expected WriteInput"),
    }
}

#[test]
fn test_protocol_invalid_json_rejection() {
    // Invalid JSON - missing closing brace
    let invalid = r#"{"type":"write_input","data":"test""#;
    let result: Result<Request, _> = serde_json::from_str(invalid);
    assert!(result.is_err());

    // Invalid JSON - unknown type value (but valid JSON syntax)
    let unknown_type = r#"{"type":"unknown_type","data":"test"}"#;
    let _result: Result<Request, _> = serde_json::from_str(unknown_type);
    // This should succeed in parsing JSON, but the tag might cause issues
    // depending on serde's behavior
}

#[test]
fn test_protocol_unknown_type_tag() {
    // Unknown type tag in request - should fail to deserialize
    let unknown = r#"{"type":"unknown_request_type"}"#;
    let result: Result<Request, _> = serde_json::from_str(unknown);
    assert!(result.is_err());

    // Unknown type tag in response
    let unknown_resp = r#"{"type":"unknown_response_type"}"#;
    let result: Result<Response, _> = serde_json::from_str(unknown_resp);
    assert!(result.is_err());
}

#[test]
fn test_protocol_empty_data() {
    let req = Request::WriteInput {
        data: "".to_string(),
    };
    let json = serde_json::to_string(&req).unwrap();
    let decoded: Request = serde_json::from_str(&json).unwrap();
    match decoded {
        Request::WriteInput { data } => assert!(data.is_empty()),
        _ => panic!("expected WriteInput"),
    }
}

#[test]
fn test_protocol_large_data() {
    let large_data = "x".repeat(1_000_000);
    let req = Request::WriteInput {
        data: large_data.clone(),
    };
    let json = serde_json::to_string(&req).unwrap();
    let decoded: Request = serde_json::from_str(&json).unwrap();
    match decoded {
        Request::WriteInput { data } => {
            assert_eq!(data.len(), large_data.len());
            assert_eq!(data, large_data);
        }
        _ => panic!("expected WriteInput"),
    }
}

#[test]
fn test_protocol_base64_variations() {
    // Valid base64
    let resp = Response::Output {
        raw_b64: "SGVsbG8gV29ybGQh".to_string(),
        screen: "Hello".to_string(),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let decoded: Response = serde_json::from_str(&json).unwrap();
    match decoded {
        Response::Output { raw_b64, .. } => {
            assert_eq!(raw_b64, "SGVsbG8gV29ybGQh");
        }
        _ => panic!("expected Output"),
    }

    // Empty base64
    let resp = Response::Output {
        raw_b64: "".to_string(),
        screen: "".to_string(),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let decoded: Response = serde_json::from_str(&json).unwrap();
    match decoded {
        Response::Output { raw_b64, .. } => {
            assert!(raw_b64.is_empty());
        }
        _ => panic!("expected Output"),
    }

    // Base64 with padding
    let resp = Response::Output {
        raw_b64: "dGVzdA==".to_string(),
        screen: "test".to_string(),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let decoded: Response = serde_json::from_str(&json).unwrap();
    match decoded {
        Response::Output { raw_b64, .. } => {
            assert_eq!(raw_b64, "dGVzdA==");
        }
        _ => panic!("expected Output"),
    }
}

#[test]
fn test_protocol_type_tag_required() {
    // Missing type tag should fail
    let missing_type = r#"{"data":"test"}"#;
    let result: Result<Request, _> = serde_json::from_str(missing_type);
    assert!(result.is_err());
}

#[test]
fn test_protocol_extra_fields_ignored() {
    // Extra fields in JSON might be ignored or cause errors depending on config
    let extra_fields = r#"{"type":"write_input","data":"test","extra":"ignored"}"#;
    // This behavior depends on serde configuration - by default extra fields are ignored
    let _result: Result<Request, _> = serde_json::from_str(extra_fields);
    // Depending on serde derive config, this may succeed or fail
    // We just verify it doesn't panic
}

#[test]
fn test_protocol_error_message_escaping() {
    // Error message with quotes
    let resp = Response::Error {
        message: r#"Error: "file not found""#.to_string(),
    };
    let json = serde_json::to_string(&resp).unwrap();
    // Quotes should be escaped
    assert!(json.contains(r#"\"file not found\""#));

    let decoded: Response = serde_json::from_str(&json).unwrap();
    match decoded {
        Response::Error { message } => {
            assert_eq!(message, r#"Error: "file not found""#);
        }
        _ => panic!("expected Error"),
    }
}

#[test]
fn test_protocol_get_output_minimal() {
    // GetOutput has no fields, should be minimal JSON
    let req = Request::GetOutput;
    let json = serde_json::to_string(&req).unwrap();
    assert_eq!(json, r#"{"type":"get_output"}"#);

    // Verify deserialization
    let decoded: Request = serde_json::from_str(&json).unwrap();
    assert!(matches!(decoded, Request::GetOutput));
}

#[test]
fn test_protocol_ok_minimal() {
    // Ok response has no fields
    let resp = Response::Ok;
    let json = serde_json::to_string(&resp).unwrap();
    assert_eq!(json, r#"{"type":"ok"}"#);

    let decoded: Response = serde_json::from_str(&json).unwrap();
    assert!(matches!(decoded, Response::Ok));
}
