use anyhow::{bail, Context, Result};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};

use crate::protocol::{Request, Response};

/// Length-prefixed JSON framing: [u32 LE length][JSON bytes].
pub struct IpcClient {
    stream: UnixStream,
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
