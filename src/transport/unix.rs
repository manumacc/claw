use std::{path::PathBuf, time::Instant};

use anyhow::{Context, Result};
use async_trait::async_trait;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
};
use tracing::{debug, error, info, warn};

use crate::{
    api::RequestHandler,
    ipc::{RequestEnvelope, ResponseEnvelope},
    transport::{TransportClient, TransportServer},
};

#[derive(Debug, Clone)]
pub struct UnixSocketServer {
    socket_path: PathBuf,
}

impl UnixSocketServer {
    pub fn new(socket_path: PathBuf) -> Self {
        Self { socket_path }
    }
}

#[async_trait]
impl TransportServer for UnixSocketServer {
    async fn serve<H>(&self, handler: H) -> Result<()>
    where
        H: RequestHandler,
    {
        if self.socket_path.exists() {
            debug!(socket_path = %self.socket_path.display(), "removing stale socket");
            std::fs::remove_file(&self.socket_path)
                .with_context(|| format!("removing stale {}", self.socket_path.display()))?;
        }

        let listener = UnixListener::bind(&self.socket_path)
            .with_context(|| format!("binding {}", self.socket_path.display()))?;
        info!(socket_path = %self.socket_path.display(), "listening on unix socket");

        loop {
            tokio::select! {
                result = listener.accept() => {
                    let (stream, _) = result?;
                    let handler = handler.clone();
                    tokio::spawn(async move {
                        if let Err(error) = handle_connection(stream, handler).await {
                            error!(error = %error, "connection error");
                        }
                    });
                }
                _ = tokio::signal::ctrl_c() => {
                    info!("received ctrl-c");
                    break;
                }
            }
        }

        if let Err(error) = std::fs::remove_file(&self.socket_path) {
            warn!(
                socket_path = %self.socket_path.display(),
                error = %error,
                "failed to remove unix socket"
            );
        } else {
            info!(socket_path = %self.socket_path.display(), "removed unix socket");
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct UnixSocketClient {
    socket_path: PathBuf,
}

impl UnixSocketClient {
    pub fn new(socket_path: PathBuf) -> Self {
        Self { socket_path }
    }
}

#[async_trait]
impl TransportClient for UnixSocketClient {
    async fn send(&self, envelope: RequestEnvelope) -> Result<ResponseEnvelope> {
        let mut stream = UnixStream::connect(&self.socket_path)
            .await
            .with_context(|| format!("connecting to {}", self.socket_path.display()))?;
        let raw = serde_json::to_vec(&envelope)?;
        stream.write_all(&raw).await?;
        stream.write_all(b"\n").await?;
        stream.shutdown().await?;

        let mut response = Vec::new();
        stream.read_to_end(&mut response).await?;
        Ok(serde_json::from_slice(&response)?)
    }
}

async fn handle_connection<H>(stream: UnixStream, handler: H) -> Result<()>
where
    H: RequestHandler,
{
    let started = Instant::now();
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    let envelope: RequestEnvelope = serde_json::from_str(&line)?;
    let id = envelope.id.clone();
    let request_kind = envelope.request.kind();

    debug!(request_id = %id, request_kind = %request_kind, "received request");

    let response = match handler.handle(envelope.request).await {
        Ok(result) => {
            info!(
                request_id = %id,
                request_kind = %request_kind,
                ok = true,
                elapsed_ms = started.elapsed().as_millis(),
                "handled request"
            );
            ResponseEnvelope::ok(id, result)
        }
        Err(error) => {
            let error = error.to_string();
            warn!(
                request_id = %id,
                request_kind = %request_kind,
                ok = false,
                elapsed_ms = started.elapsed().as_millis(),
                error = %error,
                "request failed"
            );
            ResponseEnvelope::error(id, error)
        }
    };

    let mut stream = reader.into_inner();
    let raw = serde_json::to_vec(&response)?;
    stream.write_all(&raw).await?;
    stream.write_all(b"\n").await?;
    stream.shutdown().await?;
    Ok(())
}
