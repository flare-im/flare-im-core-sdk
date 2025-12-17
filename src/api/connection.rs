//! 连接管理 API 实现

use crate::api::traits::ConnectionApi;
use crate::api::{FlareIMClient, LoginResult};
use anyhow::{Context, Result};
use std::sync::Arc;
use std::time::Instant;
use tokio::time::{Duration, sleep};
use tracing::{debug, info, warn};

impl ConnectionApi for FlareIMClient {
    async fn login(&self, user_id: &str, token: &str) -> Result<LoginResult> {
        let login_start = Instant::now();
        info!(user_id = %user_id, "Logging in");

        {
            let mut config = self.config.write().await;
            config.token = Some(token.to_string());
            config.user_id = user_id.to_string();
        }

        let (protocols_opt, protocol_opt, server_url, connect_timeout) = {
            let config_guard = self.config.read().await;
            (
                config_guard.protocols.clone(),
                config_guard.protocol,
                config_guard.server_url.clone(),
                config_guard.connect_timeout,
            )
        };

        let mut event_rx = self.event_bus.subscribe();

        let connect_future: std::pin::Pin<
            Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send>,
        > = if let Some(protocols) = protocols_opt.clone() {
            Box::pin(self.connection.connect_with_race(protocols))
        } else {
            let protocol = protocol_opt
                .unwrap_or(flare_core::common::config_types::TransportProtocol::WebSocket);
            Box::pin(self.connection.connect(protocol))
        };

        let connect_result = connect_future.await;
        match connect_result {
            Ok(()) => {
                info!("Connection established successfully");
            }
            Err(e) => {
                let error_msg = format!("{}", e);
                if error_msg.contains("timeout") || error_msg.contains("timed out") {
                    return Err(anyhow::anyhow!(
                        "Connection timeout: Unable to connect to server within {} seconds",
                        connect_timeout
                    ))
                    .context("Failed to connect to server: timeout");
                }
                return Err(anyhow::anyhow!("Connection failed: {}", error_msg))
                    .context("Failed to connect to server");
            }
        }

        let auth_check_start = std::time::Instant::now();
        let max_auth_wait = Duration::from_secs(10);

        loop {
            tokio::select! {
                event_result = tokio::time::timeout(Duration::from_millis(200), event_rx.recv()) => {
                    match event_result {
                        Ok(Ok(event)) => {
                            match event {
                                crate::infrastructure::event::Event::Connection(
                                    crate::infrastructure::event::ConnectionEvent::Authenticated
                                ) => {
                                    info!("✅ Authentication successful");
                                    break;
                                }
                                crate::infrastructure::event::Event::Connection(
                                    crate::infrastructure::event::ConnectionEvent::AuthenticationFailed(reason)
                                ) => {
                                    return Err(anyhow::anyhow!("Authentication failed: {}", reason));
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                        }
                    }
                _ = sleep(Duration::from_millis(200)) => {
                    let state = self.connection.state().await;
                    if matches!(state, crate::infrastructure::connection::ConnectionState::Authenticated) {
                        info!("✅ Authentication verified");
                        break;
                    }
                    if auth_check_start.elapsed() > max_auth_wait {
                        return Err(anyhow::anyhow!("Authentication timeout after {} seconds", max_auth_wait.as_secs()))
                            .context("Authentication failed: timeout");
                    }
                }
            }
        }

        {
            let mut uid = self.user_id.write().await;
            *uid = user_id.to_string();
        }

        self.task_scheduler
            .enable()
            .await
            .context("Failed to enable task scheduler")?;

        let task_scheduler_clone = Arc::clone(&self.task_scheduler);
        #[cfg(not(target_arch = "wasm32"))]
        use tokio::spawn as tokio_spawn;
        #[cfg(target_arch = "wasm32")]
        use tokio::task::spawn_local as tokio_spawn;

        tokio_spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            if let Err(e) = task_scheduler_clone
                .schedule_task_by_name("SessionSync", None)
                .await
            {
                warn!(error = %e, "Failed to schedule session sync task");
            }
        });

        info!(
            elapsed_ms = login_start.elapsed().as_millis(),
            "✅ Login completed"
        );

        Ok(LoginResult {
            user_id: user_id.to_string(),
            session_id: String::new(),
        })
    }

    async fn logout(&self) -> Result<()> {
        info!("Logging out");

        self.connection
            .disconnect()
            .await
            .context("Failed to disconnect")?;

        self.task_scheduler.disable().await;

        if let Err(e) = self.task_manager.shutdown().await {
            warn!(error = %e, "Error during task manager shutdown");
        }

        info!("Logout completed");
        Ok(())
    }

    async fn connection_state(&self) -> crate::infrastructure::connection::ConnectionState {
        self.connection.state().await
    }

    async fn set_crypto_aes256(&self, key: &[u8]) -> Result<()> {
        let crypto = crate::application::AesCrypto::new(key)?;
        self.set_crypto(Arc::new(crypto)).await
    }

    async fn set_crypto(&self, crypto: Arc<dyn crate::application::CryptoService>) -> Result<()> {
        anyhow::bail!("set_crypto: Need to implement CryptoService")
    }
}
