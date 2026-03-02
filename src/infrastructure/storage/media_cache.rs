//! 媒体缓存管理
//!
//! 管理媒体消息的本地缓存路径，实现 LRU 清理策略
//! 对标微信、Telegram、飞书的生产级别实现

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::RwLock;
use crate::domain::message::MediaAttachment;
use anyhow::Result;
use tracing::{info, warn, debug};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// 文件元数据（用于 LRU 清理）
#[derive(Debug, Clone)]
struct FileMetadata {
    /// 文件大小（字节）
    size: u64,
    /// 最后访问时间
    last_accessed: DateTime<Utc>,
    /// 文件路径
    path: PathBuf,
}

/// 媒体缓存管理器
pub struct MediaCacheManager {
    cache_root: PathBuf,
    max_cache_size: u64, // 最大缓存大小（字节）
    current_cache_size: Arc<RwLock<u64>>,
    /// 文件元数据映射（attachment_id -> FileMetadata）
    file_metadata: Arc<RwLock<HashMap<String, FileMetadata>>>,
}

impl MediaCacheManager {
    /// 从配置创建新的媒体缓存管理器
    ///
    /// # 参数
    /// * `cache_path` - 缓存主目录路径（如果为 None，使用默认路径）
    /// * `max_cache_size_mb` - 最大缓存大小（MB，0 表示不限制）
    ///
    /// # 返回
    /// * `Ok(MediaCacheManager)` - 创建成功
    /// * `Err` - 创建失败
    pub fn from_config(
        cache_path: Option<PathBuf>,
        max_cache_size_mb: u64,
    ) -> Result<Self> {
        // 确定缓存路径
        let cache_root = cache_path.unwrap_or_else(|| {
            // 默认路径：当前目录下的 flare_im_media_cache
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join("flare_im_media_cache")
        });
        
        let max_cache_size = if max_cache_size_mb == 0 {
            u64::MAX // 不限制
        } else {
            max_cache_size_mb * 1024 * 1024 // 转换为字节
        };
        
        Self::new(cache_root, max_cache_size)
    }
    
    /// 创建新的媒体缓存管理器
    ///
    /// # 参数
    /// * `cache_root` - 缓存主目录路径
    /// * `max_cache_size` - 最大缓存大小（字节）
    ///
    /// # 返回
    /// * `Ok(MediaCacheManager)` - 创建成功
    /// * `Err` - 创建失败
    pub fn new<P: AsRef<Path>>(cache_root: P, max_cache_size: u64) -> Result<Self> {
        let cache_root = cache_root.as_ref().to_path_buf();
        
        // 创建缓存目录
        std::fs::create_dir_all(&cache_root)?;
        
        // 创建子目录
        std::fs::create_dir_all(cache_root.join("images"))?;
        std::fs::create_dir_all(cache_root.join("videos"))?;
        std::fs::create_dir_all(cache_root.join("audios"))?;
        std::fs::create_dir_all(cache_root.join("files"))?;
        
        let manager = Self {
            cache_root: cache_root.clone(),
            max_cache_size,
            current_cache_size: Arc::new(RwLock::new(0)),
            file_metadata: Arc::new(RwLock::new(HashMap::new())),
        };
        
        // 初始化时扫描现有文件，重建元数据
        manager.scan_existing_files()?;
        
        info!(
            cache_root = %cache_root.display(),
            max_cache_size_mb = max_cache_size / 1024 / 1024,
            "Media cache manager initialized"
        );
        
        Ok(manager)
    }
    
    /// 扫描现有文件，重建元数据
    fn scan_existing_files(&self) -> Result<()> {
        let mut total_size = 0u64;
        let mut metadata = HashMap::new();
        
        // 扫描所有子目录
        for subdir in &["images", "videos", "audios", "files"] {
            let subdir_path = self.cache_root.join(subdir);
            if !subdir_path.exists() {
                continue;
            }
            
            for entry in std::fs::read_dir(&subdir_path)? {
                let entry = entry?;
                let path = entry.path();
                
                if path.is_file() {
                    if let Ok(metadata_info) = path.metadata() {
                        let size = metadata_info.len();
                        let modified = metadata_info
                            .modified()
                            .ok()
                            .and_then(|t| {
                                DateTime::<Utc>::from_timestamp(
                                    t.duration_since(std::time::UNIX_EPOCH)
                                        .ok()?
                                        .as_secs() as i64,
                                    0,
                                )
                            })
                            .unwrap_or_else(Utc::now);
                        
                        // 从文件名提取 attachment_id（格式：{attachment_id}.{ext}）
                        if let Some(file_name) = path.file_stem().and_then(|s| s.to_str()) {
                            total_size += size;
                            metadata.insert(
                                file_name.to_string(),
                                FileMetadata {
                                    size,
                                    last_accessed: modified,
                                    path: path.clone(),
                                },
                            );
                        }
                    }
                }
            }
        }
        
        // 更新缓存大小和元数据
        let mut size_guard = self.current_cache_size.write().unwrap();
        *size_guard = total_size;
        drop(size_guard);
        
        let mut metadata_guard = self.file_metadata.write().unwrap();
        *metadata_guard = metadata;
        drop(metadata_guard);
        
        let total_files = self.file_metadata.read().unwrap().len();
        debug!(
            total_files,
            total_size_mb = total_size / 1024 / 1024,
            "Scanned existing cache files"
        );
        
        Ok(())
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
    
    /// 检查文件是否已缓存
    pub fn is_cached(&self, attachment: &MediaAttachment) -> bool {
        let local_path = self.get_local_path(attachment);
        local_path.exists()
    }
    
    /// 获取缓存的媒体文件路径（如果存在）
    pub async fn get_cached_path(
        &self,
        attachment: &MediaAttachment,
    ) -> Option<PathBuf> {
        let local_path = self.get_local_path(attachment);
        
        if local_path.exists() {
            // 更新访问时间
            self.update_access_time(&attachment.attachment_id, &local_path);
            Some(local_path)
        } else {
            None
        }
    }
    
    /// 更新文件访问时间
    fn update_access_time(&self, attachment_id: &str, path: &Path) {
        let mut metadata = self.file_metadata.write().unwrap();
        if let Some(file_meta) = metadata.get_mut(attachment_id) {
            file_meta.last_accessed = Utc::now();
        } else {
            // 如果元数据不存在，尝试从文件系统获取
            if let Ok(file_meta) = path.metadata() {
                let size = file_meta.len();
                metadata.insert(
                    attachment_id.to_string(),
                    FileMetadata {
                        size,
                        last_accessed: Utc::now(),
                        path: path.to_path_buf(),
                    },
                );
            }
        }
    }
    
    /// 保存媒体文件到本地缓存
    pub async fn save_media(
        &self,
        attachment: &MediaAttachment,
        data: Vec<u8>,
    ) -> Result<PathBuf> {
        let local_path = self.get_local_path(attachment);
        
        // 如果文件已存在，先删除旧文件（更新缓存大小）
        if local_path.exists() {
            if let Ok(metadata) = local_path.metadata() {
                let old_size = metadata.len();
                let mut current_size = self.current_cache_size.write().unwrap();
                *current_size = current_size.saturating_sub(old_size);
            }
        }
        
        // 确保目录存在
        if let Some(parent) = local_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        
        // 写入文件
        tokio::fs::write(&local_path, data).await?;
        
        // 更新缓存大小和元数据
        let file_size = local_path.metadata()?.len();
        let mut current_size = self.current_cache_size.write().unwrap();
        *current_size += file_size;
        
        // 更新元数据
        let mut metadata = self.file_metadata.write().unwrap();
        metadata.insert(
            attachment.attachment_id.clone(),
            FileMetadata {
                size: file_size,
                last_accessed: Utc::now(),
                path: local_path.clone(),
            },
        );
        
        // 释放 current_size 锁，避免在 cleanup_cache 中死锁
        drop(current_size);
        drop(metadata);
        
        // 检查是否需要清理缓存
        let current_size_after = *self.current_cache_size.read().unwrap();
        if current_size_after > self.max_cache_size {
            self.cleanup_cache().unwrap();
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
        if let Some(cached_path) = self.get_cached_path(attachment).await {
            return Ok(cached_path);
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
            "image/bmp" => "bmp",
            "image/svg+xml" => "svg",
            "video/mp4" => "mp4",
            "video/quicktime" => "mov",
            "video/avi" => "avi",
            "video/webm" => "webm",
            "audio/mpeg" | "audio/mp3" => "mp3",
            "audio/mp4" | "audio/m4a" => "m4a",
            "audio/ogg" => "ogg",
            "audio/wav" => "wav",
            "audio/flac" => "flac",
            "application/pdf" => "pdf",
            "application/zip" => "zip",
            "application/x-rar-compressed" => "rar",
            "application/x-7z-compressed" => "7z",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => "docx",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => "xlsx",
            "application/vnd.openxmlformats-officedocument.presentationml.presentation" => "pptx",
            "text/plain" => "txt",
            "text/markdown" => "md",
            _ => "bin",
        }
    }
    
    /// 清理缓存（LRU 策略）
    ///
    /// 当缓存大小超过限制时，删除最久未访问的文件
    fn cleanup_cache(&self) -> Result<()> {
        let mut current_size = self.current_cache_size.write().unwrap();
        
        if *current_size <= self.max_cache_size {
            return Ok(());
        }
        
        warn!(
            current_size_mb = *current_size / 1024 / 1024,
            max_size_mb = self.max_cache_size / 1024 / 1024,
            "Cache size exceeded, starting LRU cleanup"
        );
        
        let mut metadata = self.file_metadata.write().unwrap();
        
        // 按最后访问时间排序（最旧的在前）
        let mut files: Vec<(String, FileMetadata)> = metadata.drain().collect();
        files.sort_by(|a, b| a.1.last_accessed.cmp(&b.1.last_accessed));
        
        // 计算需要释放的空间（保留 80% 的缓存空间）
        let target_size = (self.max_cache_size as f64 * 0.8) as u64;
        let mut freed_size = 0u64;
        let mut deleted_count = 0usize;
        
        // 删除最旧的文件，直到达到目标大小
        for (attachment_id, file_meta) in files.iter() {
            if *current_size - freed_size <= target_size {
                // 已经释放足够的空间，将剩余文件重新加入元数据
                for (id, meta) in files.iter().skip(deleted_count) {
                    metadata.insert(id.clone(), meta.clone());
                }
                break;
            }
            
            // 尝试删除文件
            if file_meta.path.exists() {
                match std::fs::remove_file(&file_meta.path) {
                    Ok(_) => {
                        freed_size += file_meta.size;
                        deleted_count += 1;
                        debug!(
                            attachment_id = %attachment_id,
                            path = %file_meta.path.display(),
                            size = file_meta.size,
                            "Deleted cached file (LRU cleanup)"
                        );
                    }
                    Err(e) => {
                        warn!(
                            attachment_id = %attachment_id,
                            path = %file_meta.path.display(),
                            error = %e,
                            "Failed to delete cached file"
                        );
                        // 即使删除失败，也保留元数据（文件可能已被外部删除）
                    }
                }
            }
        }
        
        // 更新缓存大小
        *current_size = current_size.saturating_sub(freed_size);
        
        info!(
            deleted_files = deleted_count,
            freed_size_mb = freed_size / 1024 / 1024,
            remaining_size_mb = *current_size / 1024 / 1024,
            "LRU cache cleanup completed"
        );
        
        Ok(())
    }
    
    /// 获取缓存统计信息
    pub fn get_cache_stats(&self) -> CacheStats {
        let current_size = *self.current_cache_size.read().unwrap();
        let file_count = self.file_metadata.read().unwrap().len();
        
        CacheStats {
            current_size,
            max_size: self.max_cache_size,
            file_count,
            usage_percent: if self.max_cache_size > 0 {
                (current_size as f64 / self.max_cache_size as f64) * 100.0
            } else {
                0.0
            },
        }
    }
    
    /// 清除所有缓存
    pub fn clear_cache(&self) -> Result<()> {
        let mut deleted_count = 0usize;
        let mut total_freed = 0u64;
        
        // 删除所有缓存文件
        let mut metadata = self.file_metadata.write().unwrap();
        for (attachment_id, file_meta) in metadata.iter() {
            if file_meta.path.exists() {
                match std::fs::remove_file(&file_meta.path) {
                    Ok(_) => {
                        total_freed += file_meta.size;
                        deleted_count += 1;
                    }
                    Err(e) => {
                        warn!(
                            attachment_id = %attachment_id,
                            path = %file_meta.path.display(),
                            error = %e,
                            "Failed to delete cached file during clear"
                        );
                    }
                }
            }
        }
        
        // 清空元数据
        metadata.clear();
        
        // 重置缓存大小
        let mut current_size = self.current_cache_size.write().unwrap();
        *current_size = 0;
        
        info!(
            deleted_files = deleted_count,
            freed_size_mb = total_freed / 1024 / 1024,
            "Cache cleared"
        );
        
        Ok(())
    }
    
    /// 删除指定的缓存文件
    pub fn delete_cached_file(
        &self,
        attachment: &MediaAttachment,
    ) -> Result<bool> {
        let local_path = self.get_local_path(attachment);
        
        if !local_path.exists() {
            return Ok(false);
        }
        
        // 获取文件大小
        let file_size = local_path.metadata()?.len();
        
        // 删除文件
        std::fs::remove_file(&local_path)?;
        
        // 更新缓存大小和元数据
        let mut current_size = self.current_cache_size.write().unwrap();
        *current_size = current_size.saturating_sub(file_size);
        
        let mut metadata = self.file_metadata.write().unwrap();
        metadata.remove(&attachment.attachment_id);
        
        info!(
            attachment_id = %attachment.attachment_id,
            path = %local_path.display(),
            size = file_size,
            "Deleted cached file"
        );
        
        Ok(true)
    }
    
    /// 获取缓存根目录
    pub fn cache_root(&self) -> &Path {
        &self.cache_root
    }
}

/// 缓存统计信息
#[derive(Debug, Clone)]
pub struct CacheStats {
    /// 当前缓存大小（字节）
    pub current_size: u64,
    /// 最大缓存大小（字节）
    pub max_size: u64,
    /// 文件数量
    pub file_count: usize,
    /// 使用百分比
    pub usage_percent: f64,
}
