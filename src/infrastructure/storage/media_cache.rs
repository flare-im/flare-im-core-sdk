//! 媒体缓存管理
//!
//! 管理媒体消息的本地缓存路径
//! 对标微信、Telegram、飞书的生产级别实现

use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::domain::message::MediaAttachment;
use anyhow::Result;
use tracing::{info, warn, error};

/// 媒体缓存管理器
pub struct MediaCacheManager {
    cache_root: PathBuf,
    max_cache_size: u64, // 最大缓存大小（字节）
    current_cache_size: Arc<RwLock<u64>>,
}

impl MediaCacheManager {
    /// 创建新的媒体缓存管理器
    pub fn new<P: AsRef<Path>>(cache_root: P, max_cache_size: u64) -> Result<Self> {
        let cache_root = cache_root.as_ref().to_path_buf();
        
        // 创建缓存目录
        std::fs::create_dir_all(&cache_root)?;
        
        // 创建子目录
        std::fs::create_dir_all(cache_root.join("images"))?;
        std::fs::create_dir_all(cache_root.join("videos"))?;
        std::fs::create_dir_all(cache_root.join("audios"))?;
        std::fs::create_dir_all(cache_root.join("files"))?;
        
        Ok(Self {
            cache_root,
            max_cache_size,
            current_cache_size: Arc::new(RwLock::new(0)),
        })
    }
    
    /// 获取媒体文件的本地缓存路径
    pub fn get_local_path(
        &self,
        attachment: &MediaAttachment,
    ) -> PathBuf {
        let file_extension = self.get_file_extension(&attachment.mime_type);
        let file_name = format!("{}.{}", attachment.attachment_id, file_extension);
        
        let subdir = match attachment.attachment_type.as_str() {
            "image" => "images",
            "video" => "videos",
            "audio" => "audios",
            _ => "files",
        };
        
        self.cache_root.join(subdir).join(file_name)
    }
    
    /// 保存媒体文件到本地缓存
    pub async fn save_media(
        &self,
        attachment: &MediaAttachment,
        data: Vec<u8>,
    ) -> Result<PathBuf> {
        let local_path = self.get_local_path(attachment);
        
        // 确保目录存在
        if let Some(parent) = local_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        
        // 写入文件
        tokio::fs::write(&local_path, data).await?;
        
        // 更新缓存大小
        let file_size = local_path.metadata()?.len();
        let mut current_size = self.current_cache_size.write().await;
        *current_size += file_size;
        
        // 检查是否需要清理缓存
        if *current_size > self.max_cache_size {
            self.cleanup_cache().await?;
        }
        
        info!(
            attachment_id = %attachment.attachment_id,
            local_path = %local_path.display(),
            size = file_size,
            "Media file saved to local cache"
        );
        
        Ok(local_path)
    }
    
    /// 从 URL 下载并保存媒体文件
    pub async fn download_and_save(
        &self,
        attachment: &MediaAttachment,
    ) -> Result<PathBuf> {
        // 检查是否已缓存
        let local_path = self.get_local_path(attachment);
        if local_path.exists() {
            info!(
                attachment_id = %attachment.attachment_id,
                "Media file already cached"
            );
            return Ok(local_path);
        }
        
        // 下载文件
        let response = reqwest::get(&attachment.url).await?;
        let data = response.bytes().await?.to_vec();
        
        // 保存到本地
        self.save_media(attachment, data).await
    }
    
    /// 获取文件扩展名
    fn get_file_extension(&self, mime_type: &str) -> &str {
        match mime_type {
            "image/jpeg" | "image/jpg" => "jpg",
            "image/png" => "png",
            "image/gif" => "gif",
            "image/webp" => "webp",
            "video/mp4" => "mp4",
            "video/quicktime" => "mov",
            "audio/mpeg" => "mp3",
            "audio/mp4" => "m4a",
            "audio/ogg" => "ogg",
            "application/pdf" => "pdf",
            "application/zip" => "zip",
            _ => "bin",
        }
    }
    
    /// 清理缓存（LRU 策略）
    async fn cleanup_cache(&self) -> Result<()> {
        warn!("Cache size exceeded, starting cleanup");
        
        // TODO: 实现 LRU 清理策略
        // 1. 按最后访问时间排序
        // 2. 删除最旧的文件
        // 3. 更新缓存大小
        
        Ok(())
    }
    
    /// 获取缓存统计信息
    pub async fn get_cache_stats(&self) -> CacheStats {
        let current_size = *self.current_cache_size.read().await;
        
        CacheStats {
            current_size,
            max_size: self.max_cache_size,
            usage_percent: (current_size as f64 / self.max_cache_size as f64) * 100.0,
        }
    }
    
    /// 清除所有缓存
    pub async fn clear_cache(&self) -> Result<()> {
        // 删除所有缓存文件
        for entry in std::fs::read_dir(&self.cache_root)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                std::fs::remove_file(path)?;
            }
        }
        
        // 重置缓存大小
        let mut current_size = self.current_cache_size.write().await;
        *current_size = 0;
        
        info!("Cache cleared");
        Ok(())
    }
}

/// 缓存统计信息
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub current_size: u64,
    pub max_size: u64,
    pub usage_percent: f64,
}
