use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DownloadManifestState {
    Initiating,
    Downloading,
    Completed,
    Failed,
    Aborted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaDownloadPartVo {
    pub download_id: String,
    pub part_number: u32,
    pub offset: u64,
    pub size: u64,
    pub sha256: Option<String>,
    pub downloaded_bytes: u64,
}

impl MediaDownloadPartVo {
    pub fn partial(
        download_id: impl Into<String>,
        part_number: u32,
        offset: u64,
        size: u64,
        downloaded_bytes: u64,
        sha256: Option<String>,
    ) -> Self {
        Self {
            download_id: download_id.into(),
            part_number,
            offset,
            size,
            sha256,
            downloaded_bytes: downloaded_bytes.min(size),
        }
    }

    pub fn completed(
        download_id: impl Into<String>,
        part_number: u32,
        offset: u64,
        size: u64,
        sha256: Option<String>,
    ) -> Self {
        Self::partial(download_id, part_number, offset, size, size, sha256)
    }

    pub fn record_progress(&mut self, downloaded_bytes: u64) -> bool {
        let downloaded_bytes = downloaded_bytes.min(self.size);
        if downloaded_bytes <= self.downloaded_bytes {
            return false;
        }

        self.downloaded_bytes = downloaded_bytes;
        true
    }

    pub fn resume_offset(&self) -> u64 {
        self.offset
            .saturating_add(self.downloaded_bytes.min(self.size))
    }

    pub fn is_complete(&self) -> bool {
        self.downloaded_bytes >= self.size
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaDownloadManifestVo {
    pub download_id: String,
    pub file_id: String,
    pub tenant_id: String,
    pub user_id: String,
    pub file_name: String,
    pub mime_type: String,
    pub file_size: u64,
    pub part_size: u32,
    pub total_parts: u32,
    pub content_sha256: Option<String>,
    pub state: DownloadManifestState,
    pub local_temp_path: String,
    pub local_final_path: Option<String>,
    pub last_error_code: Option<String>,
    pub last_error_message: Option<String>,
    pub expires_at_ms: Option<i64>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl MediaDownloadManifestVo {
    pub fn completed_bytes(&self, parts: &[MediaDownloadPartVo]) -> u64 {
        parts
            .iter()
            .filter(|part| part.download_id == self.download_id)
            .map(|part| part.downloaded_bytes.min(part.size))
            .sum::<u64>()
            .min(self.file_size)
    }

    pub fn next_resume_offset(&self, parts: &[MediaDownloadPartVo]) -> u64 {
        parts
            .iter()
            .filter(|part| part.download_id == self.download_id)
            .filter(|part| !part.is_complete())
            .min_by_key(|part| part.part_number)
            .map(MediaDownloadPartVo::resume_offset)
            .unwrap_or(self.file_size)
    }

    pub fn mark_completed_if_ready(
        &mut self,
        parts: &[MediaDownloadPartVo],
        local_final_path: impl Into<String>,
        updated_at_ms: i64,
    ) -> bool {
        let mut seen_parts = 0_u32;
        let all_complete = parts
            .iter()
            .filter(|part| part.download_id == self.download_id)
            .inspect(|_| {
                seen_parts = seen_parts.saturating_add(1);
            })
            .all(MediaDownloadPartVo::is_complete);

        if seen_parts != self.total_parts || !all_complete {
            return false;
        }

        self.state = DownloadManifestState::Completed;
        self.local_final_path = Some(local_final_path.into());
        self.updated_at_ms = self.updated_at_ms.max(updated_at_ms);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{DownloadManifestState, MediaDownloadManifestVo, MediaDownloadPartVo};

    fn manifest() -> MediaDownloadManifestVo {
        MediaDownloadManifestVo {
            download_id: "download-1".to_string(),
            file_id: "file-1".to_string(),
            tenant_id: "tenant-1".to_string(),
            user_id: "user-1".to_string(),
            file_name: "image.jpg".to_string(),
            mime_type: "image/jpeg".to_string(),
            file_size: 10,
            part_size: 5,
            total_parts: 2,
            content_sha256: None,
            state: DownloadManifestState::Downloading,
            local_temp_path: "/tmp/file-1.part".to_string(),
            local_final_path: None,
            last_error_code: None,
            last_error_message: None,
            expires_at_ms: None,
            created_at_ms: 1,
            updated_at_ms: 1,
        }
    }

    #[test]
    fn download_part_progress_is_monotonic_and_resumable() {
        let mut part = MediaDownloadPartVo {
            download_id: "download-1".to_string(),
            part_number: 1,
            offset: 5,
            size: 5,
            sha256: None,
            downloaded_bytes: 0,
        };

        assert!(part.record_progress(3));
        assert_eq!(part.resume_offset(), 8);

        assert!(!part.record_progress(2));
        assert_eq!(part.downloaded_bytes, 3);
        assert_eq!(part.resume_offset(), 8);

        assert!(part.record_progress(99));
        assert_eq!(part.downloaded_bytes, 5);
        assert!(part.is_complete());
    }

    #[test]
    fn manifest_resumes_from_first_incomplete_part() {
        let manifest = manifest();
        let parts = vec![
            MediaDownloadPartVo::completed("download-1", 0, 0, 5, None),
            MediaDownloadPartVo::partial("download-1", 1, 5, 5, 2, None),
        ];

        assert_eq!(manifest.completed_bytes(&parts), 7);
        assert_eq!(manifest.next_resume_offset(&parts), 7);
    }

    #[test]
    fn manifest_completes_only_when_all_parts_complete() {
        let mut manifest = manifest();
        let parts = vec![
            MediaDownloadPartVo::completed("download-1", 0, 0, 5, None),
            MediaDownloadPartVo::partial("download-1", 1, 5, 5, 4, None),
        ];

        assert!(!manifest.mark_completed_if_ready(&parts, "/downloads/image.jpg", 20));
        assert_eq!(manifest.state, DownloadManifestState::Downloading);

        let parts = vec![
            MediaDownloadPartVo::completed("download-1", 0, 0, 5, None),
            MediaDownloadPartVo::completed("download-1", 1, 5, 5, None),
        ];

        assert!(manifest.mark_completed_if_ready(&parts, "/downloads/image.jpg", 21));
        assert_eq!(manifest.state, DownloadManifestState::Completed);
        assert_eq!(
            manifest.local_final_path.as_deref(),
            Some("/downloads/image.jpg")
        );
    }
}
