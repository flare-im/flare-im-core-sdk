//! 会话命令处理器（Session Command Handler）
//!
//! 职责：编排登录/登出/连接相关的写操作，调用领域服务处理业务逻辑

use std::sync::Arc;
use tokio::sync::Mutex;
use crate::application::fsm::FsmManager;
use crate::domain::repository::{EventStore, ReadStore};
use crate::domain::service::SessionDomainService;
use crate::infrastructure::network::NetworkClient;
use crate::config::SdkConfig;
use crate::application::commands::*;
use crate::application::handlers::NetworkMessageDispatcher;
use crate::infrastructure::event_bus::EventBus;
use crate::application::extension::ExtensionRegistry;
use crate::domain::message_queue::MessageQueue;

/// 会话命令处理器
pub struct SessionCommandHandler {
    fsm: Arc<FsmManager>,
    event_store: Arc<dyn EventStore>,
    read_store: Arc<dyn ReadStore>,
    config: SdkConfig,
    network: Arc<Mutex<Option<NetworkClient>>>,
    domain_service: SessionDomainService,
    event_bus: Arc<EventBus>,
    extension_registry: Arc<ExtensionRegistry>,
    message_queue: Option<Arc<MessageQueue>>, // 可选的 MessageQueue（如果提供，使用它；否则创建新的）
}

impl SessionCommandHandler {
    pub fn new(
        fsm: Arc<FsmManager>,
        event_store: Arc<dyn EventStore>,
        read_store: Arc<dyn ReadStore>,
        config: SdkConfig,
        network: Arc<Mutex<Option<NetworkClient>>>,
        event_bus: Arc<EventBus>,
        extension_registry: Arc<ExtensionRegistry>,
        message_queue: Option<Arc<MessageQueue>>,
    ) -> Self {
        Self {
            fsm,
            event_store,
            read_store,
            config,
            network,
            domain_service: SessionDomainService::new(),
            event_bus,
            extension_registry,
            message_queue,
        }
    }
    
    /// 设置网络客户端
    pub async fn set_network_client(&self, client: NetworkClient) {
        let mut network = self.network.lock().await;
        *network = Some(client);
    }
    
    /// 处理登录命令
    pub async fn handle_login(&self, cmd: LoginCommand) -> anyhow::Result<()> {
        // 使用领域服务验证凭证
        self.domain_service.validate_credentials(&cmd.user_id, &cmd.token)?;
        
        // 通过 FSM 开始登录
        self.fsm.session_start_login().await?;
        
        // 登录成功，通过 FSM 更新状态
        self.fsm.session_login_success(cmd.user_id.clone(), cmd.token.clone()).await?;
        
        // 登录成功后自动连接
        self.handle_connect(ConnectCommand).await?;
        
        Ok(())
    }
    
    /// 处理登出命令
    pub async fn handle_logout(&self, _cmd: LogoutCommand) -> anyhow::Result<()> {
        // 断开连接
        if let Some(mut network) = self.network.lock().await.take() {
            network.disconnect().await?;
        }
        self.fsm.connection_disconnect().await?;
        
        // 通过 FSM 登出
        self.fsm.session_logout().await?;
        
        Ok(())
    }
    
    /// 处理连接命令
    pub async fn handle_connect(&self, _cmd: ConnectCommand) -> anyhow::Result<()> {
        // 检查 Session 状态
        let session_state = self.fsm.session_state().await;
        use crate::domain::session::SessionState;
        if session_state != SessionState::Active {
            return Err(anyhow::anyhow!("Session is not active, cannot connect"));
        }
        
        // 检查连接状态，如果已经连接，直接返回成功
        let connection_state = self.fsm.connection_state().await;
        use crate::domain::connection::ConnectionState;
        if connection_state == ConnectionState::Online {
            tracing::debug!("Already connected, skipping connect");
            return Ok(());
        }
        
        // 通过 FSM 开始连接
        self.fsm.connection_start_connect().await?;
        
        // 创建或使用现有的消息队列
        let message_queue = if let Some(ref queue) = self.message_queue {
            queue.clone()
        } else {
            // 如果没有提供 MessageQueue，创建新的（向后兼容）
            Arc::new(crate::domain::message_queue::MessageQueue::new())
        };
        let (mut network_client, message_rx, connection_rx, _ack_rx) = NetworkClient::new_with_queue(message_queue.clone());
        
        // 创建并启动网络消息分发器
        let dispatcher = NetworkMessageDispatcher::new(
            message_queue.clone(),
            self.read_store.clone(),
            self.event_bus.clone(),
            self.fsm.clone(),
            self.extension_registry.clone(),
        );
        dispatcher.start(message_rx); // 启动分发器（后台任务）
        
        // 从 FSM 获取 user_id 和 token
        let (user_id_opt, token_opt) = self.fsm.session_info().await;
        let user_id = user_id_opt.ok_or_else(|| anyhow::anyhow!("User not logged in"))?;
        let token = token_opt.ok_or_else(|| anyhow::anyhow!("Token not available"))?;
        
        // 连接到服务器
        let flare_core_config = self.config.to_flare_core_config()?;
        let server_url = flare_core_config.server_url.clone();
        
        tracing::info!("🔌 连接到服务器: {}", server_url);
        
        network_client.connect_with_config(
            server_url,
            user_id,
            token,
            Some(flare_core_config),
        ).await?;
        
        // 启动连接事件监听
        let fsm_clone = self.fsm.clone();
        let mut connection_rx_mut = connection_rx;
        tokio::spawn(async move {
            use crate::infrastructure::network::ConnectionEvent;
            while let Some(event) = connection_rx_mut.recv().await {
                match event {
                    ConnectionEvent::Connected => {
                        tracing::info!("✅ 网络连接已建立");
                    }
                    ConnectionEvent::Disconnected => {
                        tracing::warn!("⚠️  网络连接已断开");
                    }
                    ConnectionEvent::Error(err) => {
                        tracing::error!("❌ 网络连接错误: {}", err);
                    }
                }
            }
        });
        
        // 等待连接建立
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        
        // 获取连接 ID
        let connection_id = network_client.connection_id()
            .ok_or_else(|| anyhow::anyhow!("Failed to get connection ID"))?;
        
        // 连接成功，通过 FSM 更新状态
        self.fsm.connection_connect_success(connection_id.clone()).await?;
        
        // 保存网络客户端
        self.set_network_client(network_client).await;
        
        tracing::info!("✅ 连接成功，connection_id: {}", connection_id);
        
        Ok(())
    }
    
    /// 处理断开连接命令
    pub async fn handle_disconnect(&self, _cmd: DisconnectCommand) -> anyhow::Result<()> {
        if let Some(mut network) = self.network.lock().await.take() {
            network.disconnect().await?;
        }
        self.fsm.connection_disconnect().await?;
        
        Ok(())
    }
}
