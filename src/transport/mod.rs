use anyhow::Result;
use async_trait::async_trait;

use crate::{
    api::RequestHandler,
    ipc::{RequestEnvelope, ResponseEnvelope},
};

pub mod unix;

#[async_trait]
pub trait TransportServer: Send + Sync {
    async fn serve<H>(&self, handler: H) -> Result<()>
    where
        H: RequestHandler;
}

#[async_trait]
pub trait TransportClient: Send + Sync {
    async fn send(&self, envelope: RequestEnvelope) -> Result<ResponseEnvelope>;
}
