//! 媒体领域服务
//!
//! 职责：处理媒体文件的上传、缓存、验证等业务逻辑
//! 领域层职责：业务规则和验证
//! 基础层职责：实际的上传操作和缓存存储（通过接口注入）

use crate::domain::message::TenantContext;
use anyhow::Result;

/// 媒体领域服务
///
/// 无状态服务，负责媒体文件的业务逻辑处理
pub struct MediaDomainService;

impl MediaDomainService {
    pub fn new() -> Self {
        Self
    }
    
    // ============================================================================
    // 文件类型检测和验证
    // ============================================================================
    
    /// 检测文件 MIME 类型
    ///
    /// 根据文件扩展名推断 MIME 类型
    pub fn detect_mime_type(&self, file_path: &str) -> Result<String> {
        use std::path::Path;
        let path = Path::new(file_path);
        let extension = path.extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("");
        
        let mime_type = match extension.to_lowercase().as_str() {
            "jpg" | "jpeg" => "image/jpeg",
            "png" => "image/png",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "bmp" => "image/bmp",
            "svg" => "image/svg+xml",
            "mp4" => "video/mp4",
            "mov" => "video/quicktime",
            "avi" => "video/x-msvideo",
            "mkv" => "video/x-matroska",
            "webm" => "video/webm",
            "mp3" => "audio/mpeg",
            "m4a" => "audio/mp4",
            "ogg" => "audio/ogg",
            "wav" => "audio/wav",
            "flac" => "audio/flac",
            "pdf" => "application/pdf",
            "zip" => "application/zip",
            "rar" => "application/x-rar-compressed",
            "7z" => "application/x-7z-compressed",
            "doc" => "application/msword",
            "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "xls" => "application/vnd.ms-excel",
            "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            "ppt" => "application/vnd.ms-powerpoint",
            "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            _ => "application/octet-stream",
        };
        
        Ok(mime_type.to_string())
    }
    
    /// 确定文件类型
    pub fn determine_file_type(&self, mime_type: &str) -> MediaFileType {
        if mime_type.starts_with("image/") {
            MediaFileType::Image
        } else if mime_type.starts_with("video/") {
            MediaFileType::Video
        } else if mime_type.starts_with("audio/") {
            MediaFileType::Audio
        } else if mime_type == "application/pdf" 
            || mime_type.starts_with("application/vnd.ms-")
            || mime_type.starts_with("application/vnd.openxmlformats-") {
            MediaFileType::Document
        } else {
            MediaFileType::Other
        }
    }
    
    // ============================================================================
    // 媒体上传领域服务（领域层，基础层实现暂时留出来）
    // ============================================================================
    
    /// 准备媒体上传上下文
    ///
    /// 领域层职责：准备上传所需的业务数据
    /// 基础层职责：实际的上传操作（暂时留出来，使用占位URL）
    ///
    /// # 参数
    /// * `file_path` - 文件路径
    /// * `file_size` - 文件大小
    /// * `mime_type` - MIME 类型
    /// * `user_id` - 用户ID
    /// * `tenant` - 租户上下文
    ///
    /// # 返回
    /// * `Ok(MediaUploadContext)` - 上传上下文
    pub fn prepare_media_upload_context(
        &self,
        file_path: &str,
        file_size: u64,
        mime_type: &str,
        user_id: &str,
        tenant: &TenantContext,
    ) -> Result<MediaUploadContext> {
        use std::path::Path;
        let path = Path::new(file_path);
        let file_name = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
        
        // 生成文件ID（使用UUID）
        let file_id = uuid::Uuid::new_v4().to_string();
        
        // 确定文件类型
        let file_type = self.determine_file_type(mime_type);
        
        // 构建上传元数据
        let mut metadata = std::collections::HashMap::new();
        metadata.insert("file_name".to_string(), file_name.clone());
        metadata.insert("original_path".to_string(), file_path.to_string());
        metadata.insert("uploaded_by".to_string(), user_id.to_string());
        metadata.insert("tenant_id".to_string(), tenant.tenant_id.clone());
        metadata.insert("uploaded_at".to_string(), chrono::Utc::now().to_rfc3339());
        
        Ok(MediaUploadContext {
            file_id,
            file_name,
            file_size,
            mime_type: mime_type.to_string(),
            file_type,
            user_id: user_id.to_string(),
            tenant: tenant.clone(),
            metadata,
        })
    }
    
    /// 生成媒体上传URL（占位实现）
    ///
    /// 注意：这是领域层的占位实现，实际的上传URL应该由基础设施层提供
    /// 基础层实现暂时留出来，这里返回占位URL
    ///
    /// # 参数
    /// * `context` - 上传上下文
    ///
    /// # 返回
    /// * `Ok(String)` - 上传URL（占位）
    pub fn generate_upload_url(&self, context: &MediaUploadContext) -> Result<String> {
        // 占位实现：生成一个占位URL
        // 实际实现中，应该调用基础设施层的上传服务
        // 基础层实现暂时留出来
        let url = format!(
            "https://example.com/media/{}/{}",
            context.file_type.as_str(),
            context.file_id
        );
        Ok(url)
    }
    
    /// 验证媒体文件
    ///
    /// 领域层职责：验证文件是否符合业务规则
    ///
    /// # 参数
    /// * `file_size` - 文件大小（字节）
    /// * `mime_type` - MIME 类型
    /// * `file_type` - 文件类型
    ///
    /// # 返回
    /// * `Ok(())` - 验证通过
    /// * `Err(Error)` - 验证失败
    pub fn validate_media_file(
        &self,
        file_size: u64,
        mime_type: &str,
        file_type: MediaFileType,
    ) -> Result<()> {
        // 验证文件大小
        const MAX_FILE_SIZE: u64 = 100 * 1024 * 1024; // 100MB
        if file_size > MAX_FILE_SIZE {
            return Err(anyhow::anyhow!(
                "File size exceeds maximum limit: {}MB", 
                MAX_FILE_SIZE / 1024 / 1024
            ));
        }
        
        // 验证文件类型
        match file_type {
            MediaFileType::Image => {
                if !mime_type.starts_with("image/") {
                    return Err(anyhow::anyhow!("Invalid image MIME type: {}", mime_type));
                }
            }
            MediaFileType::Video => {
                if !mime_type.starts_with("video/") {
                    return Err(anyhow::anyhow!("Invalid video MIME type: {}", mime_type));
                }
            }
            MediaFileType::Audio => {
                if !mime_type.starts_with("audio/") {
                    return Err(anyhow::anyhow!("Invalid audio MIME type: {}", mime_type));
                }
            }
            _ => {}
        }
        
        Ok(())
    }
    
    /// 计算媒体文件的缓存键
    ///
    /// 领域层职责：定义缓存键的生成规则
    pub fn generate_cache_key(&self, file_id: &str, file_type: MediaFileType) -> String {
        format!("media:{}:{}", file_type.as_str(), file_id)
    }
    
    /// 计算媒体文件的存储路径
    ///
    /// 领域层职责：定义存储路径的生成规则
    pub fn generate_storage_path(
        &self,
        file_type: MediaFileType,
        file_id: &str,
        file_name: &str,
    ) -> String {
        // 使用文件类型和文件ID构建路径
        // 格式：{file_type}/{file_id}/{file_name}
        format!("{}/{}/{}", file_type.as_str(), file_id, file_name)
    }
}

impl Default for MediaDomainService {
    fn default() -> Self {
        Self::new()
    }
}

/// 媒体上传上下文
///
/// 包含上传所需的所有业务信息
#[derive(Debug, Clone)]
pub struct MediaUploadContext {
    /// 文件ID（唯一标识）
    pub file_id: String,
    /// 文件名
    pub file_name: String,
    /// 文件大小（字节）
    pub file_size: u64,
    /// MIME 类型
    pub mime_type: String,
    /// 文件类型
    pub file_type: MediaFileType,
    /// 上传用户ID
    pub user_id: String,
    /// 租户上下文
    pub tenant: TenantContext,
    /// 扩展元数据
    pub metadata: std::collections::HashMap<String, String>,
}

/// 媒体文件类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaFileType {
    /// 图片
    Image,
    /// 视频
    Video,
    /// 音频
    Audio,
    /// 文档
    Document,
    /// 其他
    Other,
}

impl MediaFileType {
    /// 获取文件类型的字符串表示（用于路径）
    pub fn as_str(&self) -> &'static str {
        match self {
            MediaFileType::Image => "images",
            MediaFileType::Video => "videos",
            MediaFileType::Audio => "audios",
            MediaFileType::Document => "documents",
            MediaFileType::Other => "files",
        }
    }
    
    /// 从 MIME 类型推断文件类型
    pub fn from_mime_type(mime_type: &str) -> Self {
        if mime_type.starts_with("image/") {
            MediaFileType::Image
        } else if mime_type.starts_with("video/") {
            MediaFileType::Video
        } else if mime_type.starts_with("audio/") {
            MediaFileType::Audio
        } else if mime_type == "application/pdf" 
            || mime_type.starts_with("application/vnd.ms-")
            || mime_type.starts_with("application/vnd.openxmlformats-") {
            MediaFileType::Document
        } else {
            MediaFileType::Other
        }
    }
}
