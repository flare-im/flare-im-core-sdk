use crate::domain::models::{MediaUploadManifestVo, MediaUploadPartVo};
use crate::shared::error::Result;
use async_trait::async_trait;

#[async_trait]
pub trait UploadManifestStore: Send + Sync {
    async fn get_manifest(&self, local_upload_id: &str) -> Result<Option<MediaUploadManifestVo>>;
    async fn find_active_manifest(
        &self,
        source_locator: &str,
        file_fingerprint: &str,
    ) -> Result<Option<MediaUploadManifestVo>>;
    async fn upsert_manifest(&self, manifest: &MediaUploadManifestVo) -> Result<()>;
    async fn delete_manifest(&self, local_upload_id: &str) -> Result<()>;

    async fn list_parts(&self, local_upload_id: &str) -> Result<Vec<MediaUploadPartVo>>;
    async fn replace_parts(&self, local_upload_id: &str, parts: &[MediaUploadPartVo])
    -> Result<()>;
    async fn upsert_part(&self, part: &MediaUploadPartVo) -> Result<()>;
    async fn delete_parts(&self, local_upload_id: &str) -> Result<()>;
}
