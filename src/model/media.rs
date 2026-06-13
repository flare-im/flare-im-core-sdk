#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaAccessUrl {
    pub url: String,
    pub cdn_url: Option<String>,
}

/// [`crate::platform::ports::media::MediaServicePort::resolve_media_access`] 结果：优先本地缓存，否则返回短时远程地址。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaResolvedAccess {
    /// `"local"` 或 `"remote"`
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<MediaAccessUrl>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadedMedia {
    pub file_id: String,
    pub file_name: String,
    pub mime_type: String,
    pub size: i64,
    pub url: Option<String>,
    pub cdn_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UploadOptions {
    pub chunk_size: usize,
}

impl Default for UploadOptions {
    fn default() -> Self {
        Self {
            chunk_size: 256 * 1024,
        }
    }
}
