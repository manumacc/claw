use anyhow::Result;

use crate::{
    api::ClawApi,
    config::AppConfig,
    transport::{TransportServer, unix::UnixSocketServer},
};
use tracing::info;

pub async fn run(config: AppConfig) -> Result<()> {
    info!(
        storage_root = %config.storage_root.display(),
        socket_path = %config.socket_path.display(),
        workspace_root = %config.workspace_root.display(),
        default_provider = %config.default_provider,
        "starting daemon"
    );
    let api = ClawApi::new(config.clone())?;
    let transport = UnixSocketServer::new(config.socket_path.clone());
    let result = transport.serve(api).await;
    if result.is_ok() {
        info!("daemon stopped");
    }
    result
}
