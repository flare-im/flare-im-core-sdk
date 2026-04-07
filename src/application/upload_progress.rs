use std::sync::Arc;

#[derive(Debug, Clone)]
pub enum UploadPhase {
    Preparing,
    Uploading,
    Completing,
    Finished,
}

#[derive(Debug, Clone)]
pub struct UploadProgress {
    pub file_name: String,
    pub upload_id: String,
    pub phase: UploadPhase,
    pub uploaded_bytes: u64,
    pub total_bytes: u64,
    pub chunk_index: Option<u32>,
    pub total_chunks: Option<u32>,
}

pub type UploadProgressCallback = Arc<dyn Fn(UploadProgress) + Send + Sync>;
