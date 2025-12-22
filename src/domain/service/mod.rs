//! 领域服务（Domain Service）
//!
//! 职责：包含所有业务逻辑实现
//! 领域服务是无状态的，不依赖基础设施层

pub mod message_domain_service;
pub mod conversation_domain_service;
pub mod session_domain_service;
pub mod media_domain_service;

pub use message_domain_service::MessageDomainService;
pub use message_domain_service::{MentionInfo, MentionInfoType};
pub use conversation_domain_service::ConversationDomainService;
pub use session_domain_service::SessionDomainService;
pub use media_domain_service::MediaDomainService;
pub use media_domain_service::{MediaUploadContext, MediaFileType};
