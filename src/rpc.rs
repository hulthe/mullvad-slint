use crate::RT;
use std::sync::Arc;

use anyhow::Context;
use mullvad_management_interface::MullvadProxyClient;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct Rpc {
    rpc: Arc<Mutex<Option<MullvadProxyClient>>>,
}

impl Rpc {
    pub fn new() -> Self {
        Self {
            rpc: Arc::new(Mutex::new(None)),
        }
    }

    /// Try to execute a function with a gRPC connection.
    ///
    /// # Errors
    /// Returns `Err` if connecting to gRPC fails, or if `f` returns `Err`.
    pub async fn with_rpc<Fn, Fut, T>(&self, f: Fn) -> anyhow::Result<T>
    where
        Fn: FnOnce(MullvadProxyClient) -> Fut,
        Fut: Future<Output = anyhow::Result<T>>,
    {
        let rpc = {
            let mut rpc_option = self.rpc.clone().lock_owned().await;

            // Connect to gRPC if not already connected
            if rpc_option.is_none() {
                let rpc = MullvadProxyClient::new()
                    .await
                    .context("Failed to open RPC connection")?;
                *rpc_option = Some(rpc);
            };

            rpc_option
                .as_ref()
                .expect("We have a gRPC connection")
                .clone()
        };

        f(rpc).await
    }

    /// Shorthand for spawning a tokio task and executing [`Self::with_rpc`].
    pub fn spawn_with_rpc<Fn, Fut>(&self, f: Fn)
    where
        Fn: FnOnce(MullvadProxyClient) -> Fut,
        Fut: Future<Output = anyhow::Result<()>>,
        Fn: Send + 'static,
        Fut: Send,
    {
        let this = self.clone();

        RT.spawn(async move {
            let result = this.with_rpc(f).await;
            if let Err(e) = result {
                eprintln!("{e:#?}");
            }
        });
    }
}
