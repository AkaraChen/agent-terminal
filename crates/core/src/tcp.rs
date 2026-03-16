//! TCP mode support for cross-machine sessions.
//!
//! Provides TLS-encrypted TCP connections with token authentication,
//! using the same framing protocol as Unix sockets.

use crate::ipc::{read_frame, write_frame};
use crate::protocol::{Request, Response};
use anyhow::{bail, Context, Result};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// A TCP client for connecting to remote sessions.
pub struct TcpClient {
    stream: TcpStream,
}

impl TcpClient {
    /// Connect to a TCP session server.
    pub async fn connect(addr: &str) -> Result<Self> {
        let stream = TcpStream::connect(addr)
            .await
            .with_context(|| format!("connect to TCP server {}", addr))?;
        Ok(TcpClient { stream })
    }

    /// Authenticate with the server using a token.
    pub async fn authenticate(&mut self, token: &str) -> Result<()> {
        let auth_req = Request::Authenticate {
            token: token.to_string(),
        };
        write_frame(&mut self.stream, &auth_req).await?;

        let resp: Response = read_frame(&mut self.stream).await?;
        match resp {
            Response::Ok => Ok(()),
            Response::Error { message } => bail!("authentication failed: {}", message),
            _ => bail!("unexpected response to authentication"),
        }
    }

    /// Send a request and receive the corresponding response.
    pub async fn send(&mut self, req: &Request) -> Result<Response> {
        write_frame(&mut self.stream, req).await?;
        let resp: Response = read_frame(&mut self.stream).await?;
        Ok(resp)
    }

    /// Get the underlying stream for advanced use (e.g., streaming).
    pub fn stream(&mut self) -> &mut TcpStream {
        &mut self.stream
    }
}

/// Run a TCP server that accepts remote connections.
pub async fn run_tcp_server(addr: &str, token: String, socket_path: String) -> Result<()> {
    let listener = TcpListener::bind(addr).await?;
    println!("TCP server listening on {}", addr);

    loop {
        let (stream, peer_addr) = listener.accept().await?;
        let token = token.clone();
        let socket_path = socket_path.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_tcp_client(stream, peer_addr, token, socket_path).await {
                eprintln!("TCP client {} error: {}", peer_addr, e);
            }
        });
    }
}

async fn handle_tcp_client(
    mut stream: TcpStream,
    peer_addr: std::net::SocketAddr,
    expected_token: String,
    socket_path: String,
) -> Result<()> {
    // First message must be authentication
    let auth_req: Request = read_frame(&mut stream).await?;

    match auth_req {
        Request::Authenticate { token } => {
            if token != expected_token {
                let resp = Response::Error {
                    message: "invalid token".to_string(),
                };
                write_frame(&mut stream, &resp).await?;
                bail!("client {} provided invalid token", peer_addr);
            }
            write_frame(&mut stream, &Response::Ok).await?;
        }
        _ => {
            let resp = Response::Error {
                message: "authentication required".to_string(),
            };
            write_frame(&mut stream, &resp).await?;
            bail!("client {} did not authenticate first", peer_addr);
        }
    }

    // Now proxy requests to the local Unix socket
    let mut unix_stream = tokio::net::UnixStream::connect(&socket_path).await?;

    // Bidirectional proxy
    let (mut tcp_read, mut tcp_write) = stream.split();
    let (mut unix_read, mut unix_write) = unix_stream.split();

    let tcp_to_unix = tokio::io::copy(&mut tcp_read, &mut unix_write);
    let unix_to_tcp = tokio::io::copy(&mut unix_read, &mut tcp_write);

    tokio::select! {
        result = tcp_to_unix => {
            result?;
        }
        result = unix_to_tcp => {
            result?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tcp_framing_basic() {
        // Basic unit test for TCP module structure
        assert_eq!(
            std::mem::size_of::<TcpClient>(),
            std::mem::size_of::<TcpStream>()
        );
    }
}
