use anyhow::{bail, Context, Result};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};

use crate::protocol::{Request, Response};

/// Length-prefixed JSON framing: [u32 LE length][JSON bytes].
pub struct IpcClient {
    /// The underlying Unix stream for IPC communication.
    /// Public to allow direct reading/writing for advanced use cases like streaming.
    pub stream: UnixStream,
}

impl IpcClient {
    /// Connect to a session's Unix socket.
    pub async fn connect(socket_path: &str) -> Result<Self> {
        let stream = UnixStream::connect(socket_path)
            .await
            .with_context(|| format!("connect to socket {}", socket_path))?;
        Ok(IpcClient { stream })
    }

    /// Send a request and receive the corresponding response.
    pub async fn send(&mut self, req: &Request) -> Result<Response> {
        write_frame(&mut self.stream, req).await?;
        let resp: Response = read_frame(&mut self.stream).await?;
        Ok(resp)
    }

    /// Convenience: write input data to the PTY session.
    pub async fn write_input(&mut self, data: &str) -> Result<()> {
        let req = Request::WriteInput {
            data: data.to_string(),
        };
        match self.send(&req).await? {
            Response::Ok => Ok(()),
            Response::Error { message } => bail!("session error: {}", message),
            other => bail!("unexpected response: {:?}", other),
        }
    }

    /// Convenience: request current output from the PTY session.
    pub async fn get_output(&mut self) -> Result<(String, String)> {
        match self.send(&Request::GetOutput).await? {
            Response::Output { raw_b64, screen } => Ok((raw_b64, screen)),
            Response::Error { message } => bail!("session error: {}", message),
            other => bail!("unexpected response: {:?}", other),
        }
    }
}

/// Write a length-prefixed JSON frame to the stream.
pub async fn write_frame<T: serde::Serialize>(
    stream: &mut (impl AsyncWriteExt + Unpin),
    value: &T,
) -> Result<()> {
    let json = serde_json::to_vec(value)?;
    let len = json.len() as u32;
    stream.write_all(&len.to_le_bytes()).await?;
    stream.write_all(&json).await?;
    Ok(())
}

/// Read a length-prefixed JSON frame from the stream.
pub async fn read_frame<T: serde::de::DeserializeOwned>(
    stream: &mut (impl AsyncReadExt + Unpin),
) -> Result<T> {
    let mut len_buf = [0u8; 4];
    stream
        .read_exact(&mut len_buf)
        .await
        .context("read frame length")?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > 4 * 1024 * 1024 {
        bail!("frame too large: {} bytes", len);
    }
    let mut buf = vec![0u8; len];
    stream
        .read_exact(&mut buf)
        .await
        .context("read frame body")?;
    let value = serde_json::from_slice(&buf)?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{Request, Response};
    use tokio::io::AsyncWriteExt;
    use tokio::net::UnixListener;

    // ── write_frame / read_frame roundtrip ───────────────────────────────

    #[tokio::test]
    async fn test_write_read_frame_request_roundtrip() {
        let (mut a, mut b) = tokio::io::duplex(1024);

        let req = Request::WriteInput { data: "ls -la\n".to_string() };
        write_frame(&mut a, &req).await.unwrap();
        drop(a);

        let received: Request = read_frame(&mut b).await.unwrap();
        match received {
            Request::WriteInput { data } => assert_eq!(data, "ls -la\n"),
            _ => panic!("expected WriteInput"),
        }
    }

    #[tokio::test]
    async fn test_write_read_frame_get_output() {
        let (mut a, mut b) = tokio::io::duplex(256);

        write_frame(&mut a, &Request::GetOutput).await.unwrap();
        drop(a);

        let received: Request = read_frame(&mut b).await.unwrap();
        assert!(matches!(received, Request::GetOutput));
    }

    #[tokio::test]
    async fn test_write_read_frame_response_output() {
        let (mut a, mut b) = tokio::io::duplex(1024);

        let resp = Response::Output {
            raw_b64: "aGVsbG8=".to_string(),
            screen: "hello".to_string(),
        };
        write_frame(&mut a, &resp).await.unwrap();
        drop(a);

        let received: Response = read_frame(&mut b).await.unwrap();
        match received {
            Response::Output { raw_b64, screen } => {
                assert_eq!(raw_b64, "aGVsbG8=");
                assert_eq!(screen, "hello");
            }
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    async fn test_write_read_frame_response_error() {
        let (mut a, mut b) = tokio::io::duplex(256);

        let resp = Response::Error { message: "something went wrong".to_string() };
        write_frame(&mut a, &resp).await.unwrap();
        drop(a);

        let received: Response = read_frame(&mut b).await.unwrap();
        match received {
            Response::Error { message } => assert_eq!(message, "something went wrong"),
            _ => panic!("expected Error"),
        }
    }

    // ── frame length prefix correctness ─────────────────────────────────

    #[tokio::test]
    async fn test_write_frame_length_prefix_matches_json_size() {
        use tokio::io::AsyncReadExt;
        let (mut a, mut b) = tokio::io::duplex(1024);

        let req = Request::GetOutput;
        let expected_json = serde_json::to_vec(&req).unwrap();
        write_frame(&mut a, &req).await.unwrap();
        drop(a);

        let mut len_buf = [0u8; 4];
        b.read_exact(&mut len_buf).await.unwrap();
        let len = u32::from_le_bytes(len_buf) as usize;

        assert_eq!(len, expected_json.len());
    }

    // ── oversized frame guard ─────────────────────────────────────────────

    #[tokio::test]
    async fn test_read_frame_rejects_oversized_length() {
        let (mut a, mut b) = tokio::io::duplex(16);

        let oversized: u32 = 5 * 1024 * 1024;
        a.write_all(&oversized.to_le_bytes()).await.unwrap();
        drop(a);

        let result: anyhow::Result<Request> = read_frame(&mut b).await;
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("too large"),
            "error should mention 'too large'"
        );
    }

    // ── multiple frames sequentially ────────────────────────────────────

    #[tokio::test]
    async fn test_multiple_frames_in_sequence() {
        let (mut a, mut b) = tokio::io::duplex(4096);

        let r1 = Request::GetOutput;
        let r2 = Request::WriteInput { data: "pwd\n".to_string() };

        write_frame(&mut a, &r1).await.unwrap();
        write_frame(&mut a, &r2).await.unwrap();
        drop(a);

        let recv1: Request = read_frame(&mut b).await.unwrap();
        let recv2: Request = read_frame(&mut b).await.unwrap();

        assert!(matches!(recv1, Request::GetOutput));
        match recv2 {
            Request::WriteInput { data } => assert_eq!(data, "pwd\n"),
            _ => panic!("expected WriteInput for second frame"),
        }
    }

    // ── IpcClient over a real Unix socket ────────────────────────────────

    fn temp_socket_path() -> String {
        format!("/tmp/test-ipc-{}.sock", uuid::Uuid::new_v4())
    }

    /// Spawn a one-shot mock server: accept one connection, run `handler`, done.
    async fn with_mock_server<F, Fut>(socket_path: &str, handler: F)
    where
        F: FnOnce(tokio::net::UnixStream) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send,
    {
        let listener = UnixListener::bind(socket_path).unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            handler(stream).await;
        });
    }

    #[tokio::test]
    async fn test_ipc_client_connect_and_write_input_ok() {
        let path = temp_socket_path();
        with_mock_server(&path, |mut s| async move {
            let _req: Request = read_frame(&mut s).await.unwrap();
            write_frame(&mut s, &Response::Ok).await.unwrap();
        })
        .await;

        let mut client = IpcClient::connect(&path).await.unwrap();
        client.write_input("echo hello\n").await.unwrap();
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn test_ipc_client_write_input_error_response() {
        let path = temp_socket_path();
        with_mock_server(&path, |mut s| async move {
            let _req: Request = read_frame(&mut s).await.unwrap();
            write_frame(&mut s, &Response::Error { message: "session dead".to_string() })
                .await
                .unwrap();
        })
        .await;

        let mut client = IpcClient::connect(&path).await.unwrap();
        let err = client.write_input("data").await.unwrap_err();
        assert!(err.to_string().contains("session dead"));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn test_ipc_client_write_input_unexpected_response() {
        let path = temp_socket_path();
        with_mock_server(&path, |mut s| async move {
            let _req: Request = read_frame(&mut s).await.unwrap();
            // Return Output instead of Ok — unexpected
            write_frame(&mut s, &Response::Output {
                raw_b64: String::new(),
                screen: String::new(),
            })
            .await
            .unwrap();
        })
        .await;

        let mut client = IpcClient::connect(&path).await.unwrap();
        let err = client.write_input("data").await.unwrap_err();
        assert!(err.to_string().contains("unexpected response"));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn test_ipc_client_get_output_success() {
        let path = temp_socket_path();
        with_mock_server(&path, |mut s| async move {
            let _req: Request = read_frame(&mut s).await.unwrap();
            write_frame(&mut s, &Response::Output {
                raw_b64: "aGVsbG8=".to_string(),
                screen: "hello screen".to_string(),
            })
            .await
            .unwrap();
        })
        .await;

        let mut client = IpcClient::connect(&path).await.unwrap();
        let (raw_b64, screen) = client.get_output().await.unwrap();
        assert_eq!(raw_b64, "aGVsbG8=");
        assert_eq!(screen, "hello screen");
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn test_ipc_client_get_output_error_response() {
        let path = temp_socket_path();
        with_mock_server(&path, |mut s| async move {
            let _req: Request = read_frame(&mut s).await.unwrap();
            write_frame(&mut s, &Response::Error { message: "no output".to_string() })
                .await
                .unwrap();
        })
        .await;

        let mut client = IpcClient::connect(&path).await.unwrap();
        let err = client.get_output().await.unwrap_err();
        assert!(err.to_string().contains("no output"));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn test_ipc_client_get_output_unexpected_response() {
        let path = temp_socket_path();
        with_mock_server(&path, |mut s| async move {
            let _req: Request = read_frame(&mut s).await.unwrap();
            // Return Ok instead of Output — unexpected
            write_frame(&mut s, &Response::Ok).await.unwrap();
        })
        .await;

        let mut client = IpcClient::connect(&path).await.unwrap();
        let err = client.get_output().await.unwrap_err();
        assert!(err.to_string().contains("unexpected response"));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn test_ipc_client_connect_nonexistent_socket_fails() {
        let result = IpcClient::connect("/tmp/no-such-socket-xyz.sock").await;
        assert!(result.is_err());
    }
}
