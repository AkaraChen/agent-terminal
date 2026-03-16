use agent_terminal_core::protocol::{Request, Response};
use anyhow::{bail, Context, Result};
use tokio::net::TcpStream;

use crate::RemoteAction;

pub async fn run(addr: &str, token: &str, action: RemoteAction) -> Result<()> {
    let mut stream = TcpStream::connect(addr)
        .await
        .with_context(|| format!("connect to {}", addr))?;

    // Authenticate
    let auth_req = Request::Authenticate {
        token: token.to_string(),
    };
    tcp_write_frame(&mut stream, &auth_req).await?;

    let resp: Response = tcp_read_frame(&mut stream).await?;
    match resp {
        Response::Ok => {}
        Response::Error { message } => bail!("authentication failed: {}", message),
        _ => bail!("unexpected response"),
    }

    println!("Connected to {}", addr);

    match action {
        RemoteAction::Write { data } => {
            let req = Request::WriteInput { data };
            tcp_write_frame(&mut stream, &req).await?;
            let resp: Response = tcp_read_frame(&mut stream).await?;
            match resp {
                Response::Ok => Ok(()),
                Response::Error { message } => bail!("write failed: {}", message),
                _ => bail!("unexpected response"),
            }
        }
        RemoteAction::Dump => {
            let req = Request::GetOutput;
            tcp_write_frame(&mut stream, &req).await?;
            let resp: Response = tcp_read_frame(&mut stream).await?;
            match resp {
                Response::Output { screen, .. } => {
                    println!("{}", screen);
                    Ok(())
                }
                Response::Error { message } => bail!("dump failed: {}", message),
                _ => bail!("unexpected response"),
            }
        }
    }
}

async fn tcp_write_frame<T: serde::Serialize>(
    stream: &mut TcpStream,
    value: &T,
) -> Result<()> {
    use tokio::io::AsyncWriteExt;
    let json = serde_json::to_vec(value)?;
    let len = json.len() as u32;
    stream.write_all(&len.to_le_bytes()).await?;
    stream.write_all(&json).await?;
    Ok(())
}

async fn tcp_read_frame<T: serde::de::DeserializeOwned>(stream: &mut TcpStream) -> Result<T> {
    use tokio::io::AsyncReadExt;
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > 4 * 1024 * 1024 {
        bail!("frame too large: {} bytes", len);
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    let value = serde_json::from_slice(&buf)?;
    Ok(value)
}
