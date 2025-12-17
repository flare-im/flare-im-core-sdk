//! Frame 构建器
//!
//! 封装 flare-core 的 FrameBuilder，提供更便捷的 API

use flare_core::common::protocol::flare::core::commands::command::Type as CommandType;
use flare_core::common::protocol::{
    Command, CustomCommand, Frame, FrameBuilder as FlareFrameBuilder, MessageCommand,
    NotificationCommand, Reliability, SystemCommand,
};

/// Frame 构建器（封装 flare-core 的 FrameBuilder）
pub struct FrameBuilder {
    inner: FlareFrameBuilder,
}

impl FrameBuilder {
    /// 创建新的 Frame 构建器
    pub fn new() -> Self {
        Self {
            inner: FlareFrameBuilder::new(),
        }
    }

    /// 设置命令
    pub fn with_command(mut self, command: Command) -> Self {
        self.inner = self.inner.with_command(command);
        self
    }

    /// 设置消息命令（发送消息）
    pub fn with_message_command(mut self, command: MessageCommand) -> Self {
        self.inner = self.inner.with_command(Command {
            r#type: Some(CommandType::Message(command)),
        });
        self
    }

    /// 设置系统命令（同步、查询等）
    pub fn with_system_command(mut self, command: SystemCommand) -> Self {
        self.inner = self.inner.with_command(Command {
            r#type: Some(CommandType::System(command)),
        });
        self
    }

    /// 设置通知命令
    pub fn with_notification_command(mut self, command: NotificationCommand) -> Self {
        self.inner = self.inner.with_command(Command {
            r#type: Some(CommandType::Notification(command)),
        });
        self
    }

    /// 设置自定义命令（媒体上传/下载等）
    pub fn with_custom_command(mut self, command: CustomCommand) -> Self {
        self.inner = self.inner.with_command(Command {
            r#type: Some(CommandType::Custom(command)),
        });
        self
    }

    /// 设置消息 ID（不设置则自动生成）
    pub fn with_message_id(mut self, message_id: String) -> Self {
        self.inner = self.inner.with_message_id(message_id);
        self
    }

    /// 设置可靠性等级
    pub fn with_reliability(mut self, reliability: Reliability) -> Self {
        self.inner = self.inner.with_reliability(reliability);
        self
    }

    /// 设置时间戳（不设置则使用当前时间）
    pub fn with_timestamp(mut self, timestamp: u64) -> Self {
        self.inner = self.inner.with_timestamp(timestamp);
        self
    }

    /// 添加元数据
    pub fn with_metadata(mut self, key: String, value: Vec<u8>) -> Self {
        self.inner = self.inner.with_metadata(key, value);
        self
    }

    /// 添加字符串元数据
    pub fn with_metadata_str(mut self, key: String, value: String) -> Self {
        self.inner = self.inner.with_metadata_str(key, value);
        self
    }

    /// 构建 Frame
    pub fn build(self) -> Frame {
        self.inner.build()
    }
}

impl Default for FrameBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flare_core::common::protocol::builder::ping;

    #[test]
    fn test_frame_builder() {
        let frame = FrameBuilder::new()
            .with_system_command(ping())
            .with_reliability(Reliability::AtLeastOnce)
            .build();

        assert!(!frame.message_id.is_empty());
        assert!(frame.timestamp > 0);
        assert!(frame.command.is_some());
    }
}
