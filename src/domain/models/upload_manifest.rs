use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UploadSourceKind {
    StableFile,
    SpoolFile,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UploadManifestState {
    Initiating,
    Uploading,
    Completing,
    Completed,
    Failed,
    Aborted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DirectUploadTransportKindVo {
    SinglePut,
    MultipartPut,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaUploadPartVo {
    pub local_upload_id: String,
    pub part_number: u32,
    pub offset: u64,
    pub size: u64,
    pub sha256: String,
    pub etag: Option<String>,
    pub uploaded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaUploadManifestVo {
    pub local_upload_id: String,
    pub remote_upload_id: Option<String>,
    pub file_id: Option<String>,
    pub storage_upload_id: Option<String>,
    pub tenant_id: String,
    pub user_id: String,
    pub source_kind: UploadSourceKind,
    pub source_locator: String,
    pub file_name: String,
    pub mime_type: String,
    pub file_size: u64,
    pub part_size: u32,
    pub total_parts: u32,
    pub transport_kind: Option<DirectUploadTransportKindVo>,
    pub bucket: Option<String>,
    pub object_key: Option<String>,
    pub upload_url: Option<String>,
    pub file_fingerprint: String,
    pub head_tail_sha256: String,
    pub full_sha256: Option<String>,
    pub state: UploadManifestState,
    pub last_error_code: Option<String>,
    pub last_error_message: Option<String>,
    pub expires_at_ms: Option<i64>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}
