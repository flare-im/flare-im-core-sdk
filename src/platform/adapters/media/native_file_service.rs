//! 媒体上传与下载：直传分片、网关取链、本地缓存、附件下载到用户目录并落库。

use std::collections::HashMap;
use std::path::Path;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use super::upload_shared::{
    build_control_headers as shared_build_control_headers, build_upload_metadata,
    build_upload_parts, build_upload_parts_from_manifest, compute_bytes_fingerprints,
    infer_file_type, random_upload_id, upload_file_to_uploaded_media,
};
use async_trait::async_trait;
#[cfg(not(target_arch = "wasm32"))]
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
#[cfg(not(target_arch = "wasm32"))]
use tokio::io::AsyncWriteExt;
use tokio::io::{AsyncReadExt, AsyncSeekExt, SeekFrom};
use tokio::sync::RwLock;

use crate::application::callbacks::{
    FileDownloadProgress, FileDownloadProgressCallback, UploadPhase, UploadProgress,
    UploadProgressCallback, UserFileDownloadRequest,
};
use crate::domain::{
    DirectUploadTransportKindVo, MediaCacheAdmin, MediaCacheEntryVo, MediaCacheStore,
    MediaUploadManifestVo, UploadManifestState, UploadManifestStore, UploadSourceKind,
    UserFileDownloadStore,
};
use crate::infrastructure::transport::{
    CommitDirectUploadPartsHttpRequest, CommitDirectUploadPartsHttpResponse,
    CompleteDirectUploadHttpRequest, DeleteFileHttpRequest, DeleteFileHttpResponse,
    DirectUploadTransportKindHttp, GetDirectUploadStatusHttpResponse, GetFileUrlHttpRequest,
    GetFileUrlHttpResponse, HttpApiResponse, HttpClient, InitiateDirectUploadHttpRequest,
    InitiateDirectUploadHttpResponse, PresignDirectUploadPartsHttpRequest,
    PresignDirectUploadPartsHttpResponse, UploadFileHttpResponse, UploadedPartInfoHttp,
    unwrap_api_response,
};
use crate::model::{MediaAccessUrl, MediaResolvedAccess, UploadOptions, UploadedMedia};
use crate::platform::ports::media::{
    MediaMetadata, MediaProcessorPort, MediaServicePort, MediaSourceDescriptor, MediaSourceKind,
    MediaUploaderPort, ProcessedMedia, UploadProgressSink,
};
use crate::shared::error::{ErrorCode, FlareError, Result};

const MAX_CONCURRENT_DIRECT_UPLOAD_PARTS: usize = 4;

#[derive(Clone)]
pub struct MediaService {
    http: HttpClient,
    current_user_id: Arc<RwLock<String>>,
    upload_manifest_store: Option<Arc<dyn UploadManifestStore>>,
    media_cache_store: Option<Arc<dyn MediaCacheStore>>,
    media_cache_admin: Option<Arc<dyn MediaCacheAdmin>>,
    user_file_download_store: Option<Arc<dyn UserFileDownloadStore>>,
    /// 与 `download_key` 对应；`false` 表示取消下载。
    download_cancel_flags: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
}

struct NewUploadManifest<'a> {
    user_id: &'a str,
    source_locator: &'a str,
    file_name: &'a str,
    mime_type: &'a str,
    file_size: u64,
    file_fingerprint: &'a str,
    head_tail_sha256: &'a str,
    full_sha256: Option<String>,
}

struct UploadedDirectPart {
    part_number: u32,
    size: u64,
    sha256: String,
    etag: String,
}

#[cfg(not(target_arch = "wasm32"))]
const MAX_USER_DOWNLOAD_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[cfg(not(target_arch = "wasm32"))]
fn enforce_user_download_byte_budget(
    content_length: Option<u64>,
    downloaded_bytes: u64,
    incoming_bytes: u64,
) -> Result<()> {
    if content_length.is_some_and(|total| total > MAX_USER_DOWNLOAD_BYTES) {
        return Err(FlareError::localized(
            ErrorCode::ResourceExhausted,
            format!("download exceeds {MAX_USER_DOWNLOAD_BYTES} bytes"),
        ));
    }

    let next_total = downloaded_bytes
        .checked_add(incoming_bytes)
        .ok_or_else(|| {
            FlareError::localized(ErrorCode::ResourceExhausted, "download byte count overflow")
        })?;
    if next_total > MAX_USER_DOWNLOAD_BYTES {
        return Err(FlareError::localized(
            ErrorCode::ResourceExhausted,
            format!("download exceeds {MAX_USER_DOWNLOAD_BYTES} bytes"),
        ));
    }
    Ok(())
}

impl MediaService {
    pub fn new(
        http: HttpClient,
        current_user_id: Arc<RwLock<String>>,
        upload_manifest_store: Option<Arc<dyn UploadManifestStore>>,
        media_cache_store: Option<Arc<dyn MediaCacheStore>>,
        media_cache_admin: Option<Arc<dyn MediaCacheAdmin>>,
        user_file_download_store: Option<Arc<dyn UserFileDownloadStore>>,
    ) -> Self {
        Self {
            http,
            current_user_id,
            upload_manifest_store,
            media_cache_store,
            media_cache_admin,
            user_file_download_store,
            download_cancel_flags: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn http(&self) -> &HttpClient {
        &self.http
    }

    pub async fn upload_file_from_path_with_progress(
        &self,
        path: impl AsRef<Path>,
        options: Option<UploadOptions>,
        on_progress: Option<UploadProgressCallback>,
    ) -> Result<UploadedMedia> {
        let path = path.as_ref();
        let file_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| FlareError::localized(ErrorCode::InvalidParameter, "invalid file name"))?
            .to_string();
        let metadata = tokio::fs::metadata(path)
            .await
            .map_err(|e| FlareError::general_error(format!("read file metadata failed: {e}")))?;
        let size = i64::try_from(metadata.len())
            .map_err(|_| FlareError::general_error("file too large"))?;
        let mime = infer_mime_type(&file_name);
        let options = options.unwrap_or_default();
        self.upload_via_direct_session(path, file_name, mime, size, options, on_progress.as_ref())
            .await
    }

    pub async fn upload_image_from_path_with_progress(
        &self,
        path: impl AsRef<Path>,
        options: Option<UploadOptions>,
        on_progress: Option<UploadProgressCallback>,
    ) -> Result<UploadedMedia> {
        validate_path_mime_prefix(path.as_ref(), "image/")?;
        let media = self
            .upload_file_from_path_with_progress(path, options, on_progress)
            .await?;
        Ok(media)
    }

    pub async fn upload_video_from_path_with_progress(
        &self,
        path: impl AsRef<Path>,
        options: Option<UploadOptions>,
        on_progress: Option<UploadProgressCallback>,
    ) -> Result<UploadedMedia> {
        validate_path_mime_prefix(path.as_ref(), "video/")?;
        let media = self
            .upload_file_from_path_with_progress(path, options, on_progress)
            .await?;
        Ok(media)
    }

    pub async fn upload_bytes_with_progress(
        &self,
        bytes: Vec<u8>,
        file_name: String,
        mime_type: String,
        options: Option<UploadOptions>,
        on_progress: Option<UploadProgressCallback>,
    ) -> Result<UploadedMedia> {
        let options = options.unwrap_or_default();
        self.upload_bytes_direct(&bytes, file_name, mime_type, options, on_progress.as_ref())
            .await
    }

    pub async fn delete_file(&self, file_id: &str, hard_delete: bool) -> Result<bool> {
        let req = DeleteFileHttpRequest {
            file_id: file_id.to_string(),
            hard_delete,
        };
        let body: HttpApiResponse<DeleteFileHttpResponse> = self
            .http
            .delete_with_body("/api/v1/medias/file", &req)
            .await?;
        let data = unwrap_api_response(body, "delete file")?;
        Ok(data.success)
    }

    pub async fn get_file_url(&self, file_id: &str, expires_in: i32) -> Result<MediaAccessUrl> {
        let req = GetFileUrlHttpRequest {
            file_id: file_id.to_string(),
            expires_in,
            download: false,
            response_headers: HashMap::new(),
        };
        let body: HttpApiResponse<GetFileUrlHttpResponse> =
            self.http.post("/api/v1/medias/file-url", &req).await?;
        let data = unwrap_api_response(body, "get file url")?;
        Ok(MediaAccessUrl {
            url: data.url,
            cdn_url: data.cdn_url,
        })
    }

    /// 向网关申请短时直链，`download: true` 时服务端可返回 `Content-Disposition: attachment` 等（附件下载场景）。
    pub async fn get_temp_url_for_file_download(
        &self,
        file_id: &str,
        expires_in: i32,
    ) -> Result<MediaAccessUrl> {
        let fid = file_id.trim();
        if fid.is_empty() {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "get_temp_url_for_file_download: empty file_id",
            ));
        }
        let req = GetFileUrlHttpRequest {
            file_id: fid.to_string(),
            expires_in,
            download: true,
            response_headers: HashMap::new(),
        };
        let body: HttpApiResponse<GetFileUrlHttpResponse> =
            self.http.post("/api/v1/medias/file-url", &req).await?;
        let data = unwrap_api_response(body, "get file url (download)")?;
        Ok(MediaAccessUrl {
            url: data.url,
            cdn_url: data.cdn_url,
        })
    }

    /// 解析媒体访问方式：**优先** SQLite 对照表中且仍存在的本地文件，否则向网关请求短时 URL。
    pub async fn resolve_media_access(
        &self,
        file_id: &str,
        expires_in: i32,
    ) -> Result<MediaResolvedAccess> {
        let fid = file_id.trim();
        if fid.is_empty() {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "resolve_media_access: empty file_id",
            ));
        }

        if let Some(cache) = &self.media_cache_store
            && let Some(entry) = cache.get_cached(fid).await?
        {
            return Ok(MediaResolvedAccess {
                source: "local".to_string(),
                local_path: Some(entry.local_path),
                remote: None,
            });
        }

        let remote = self.get_file_url(fid, expires_in).await?;
        Ok(MediaResolvedAccess {
            source: "remote".to_string(),
            local_path: None,
            remote: Some(remote),
        })
    }

    /// 从网关取直链并下载落盘，写入 `media_local_cache` 对照表（供「点击后缓存」等场景）。
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn cache_remote_media(
        &self,
        file_id: &str,
        expires_in: i32,
    ) -> Result<MediaCacheEntryVo> {
        let cache = self.media_cache_store.as_ref().ok_or_else(|| {
            FlareError::localized(
                ErrorCode::ConfigurationError,
                "media cache store is not configured",
            )
        })?;

        let fid = file_id.trim();
        if fid.is_empty() {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "cache_remote_media: empty file_id",
            ));
        }

        if let Some(hit) = cache.get_cached(fid).await? {
            return Ok(hit);
        }

        let access = self.get_file_url(fid, expires_in).await?;
        let url = pick_download_url(&access);
        if url.is_empty() {
            return Err(FlareError::localized(
                ErrorCode::GeneralError,
                "cache_remote_media: empty download url",
            ));
        }

        let bytes = self.http.get_bytes_direct_url(url).await?;
        let mime = infer_mime_from_url_or_octet_stream(url, &bytes);
        cache.put_bytes(fid, &bytes, &mime).await
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn cache_remote_media(
        &self,
        _file_id: &str,
        _expires_in: i32,
    ) -> Result<MediaCacheEntryVo> {
        Err(FlareError::system(
            "cache_remote_media is not supported on wasm",
        ))
    }

    pub async fn media_cache_stats(&self) -> Result<crate::domain::MediaCacheStatsVo> {
        let admin = self.media_cache_admin.as_ref().ok_or_else(|| {
            FlareError::localized(
                ErrorCode::ConfigurationError,
                "media cache is not configured",
            )
        })?;
        admin.media_cache_stats().await
    }

    pub async fn set_media_cache_max_bytes(&self, max_bytes: u64) -> Result<()> {
        let admin = self.media_cache_admin.as_ref().ok_or_else(|| {
            FlareError::localized(
                ErrorCode::ConfigurationError,
                "media cache is not configured",
            )
        })?;
        admin.set_media_cache_max_bytes(max_bytes).await
    }

    pub async fn set_media_cache_root(&self, absolute_path: Option<&str>) -> Result<()> {
        let admin = self.media_cache_admin.as_ref().ok_or_else(|| {
            FlareError::localized(
                ErrorCode::ConfigurationError,
                "media cache is not configured",
            )
        })?;
        admin.set_media_cache_root(absolute_path).await
    }

    pub async fn clear_media_cache(&self) -> Result<()> {
        let admin = self.media_cache_admin.as_ref().ok_or_else(|| {
            FlareError::localized(
                ErrorCode::ConfigurationError,
                "media cache is not configured",
            )
        })?;
        admin.clear_media_cache().await
    }

    async fn upload_via_direct_session(
        &self,
        path: &Path,
        file_name: String,
        mime_type: String,
        size: i64,
        options: UploadOptions,
        on_progress: Option<&UploadProgressCallback>,
    ) -> Result<UploadedMedia> {
        let user_id = self.current_user_id.read().await.clone();
        let file_type = infer_file_type(&mime_type);
        let source_locator = path.to_string_lossy().to_string();
        let (file_fingerprint, head_tail_sha256, full_sha256) =
            compute_file_fingerprints(path).await?;

        let mut manifest = if let Some(store) = &self.upload_manifest_store {
            if let Some(existing) = store
                .find_active_manifest(&source_locator, &file_fingerprint)
                .await?
            {
                existing
            } else {
                self.new_manifest(NewUploadManifest {
                    user_id: &user_id,
                    source_locator: &source_locator,
                    file_name: &file_name,
                    mime_type: &mime_type,
                    file_size: size as u64,
                    file_fingerprint: &file_fingerprint,
                    head_tail_sha256: &head_tail_sha256,
                    full_sha256: full_sha256.clone(),
                })
            }
        } else {
            self.new_manifest(NewUploadManifest {
                user_id: &user_id,
                source_locator: &source_locator,
                file_name: &file_name,
                mime_type: &mime_type,
                file_size: size as u64,
                file_fingerprint: &file_fingerprint,
                head_tail_sha256: &head_tail_sha256,
                full_sha256: full_sha256.clone(),
            })
        };

        if manifest.remote_upload_id.is_none() {
            emit_progress(
                on_progress,
                UploadProgress {
                    file_name: file_name.clone(),
                    upload_id: manifest.local_upload_id.clone(),
                    phase: UploadPhase::Preparing,
                    uploaded_bytes: 0,
                    total_bytes: size as u64,
                    chunk_index: None,
                    total_chunks: None,
                },
            );
            let headers = self.build_control_headers(&manifest.local_upload_id);
            let req = InitiateDirectUploadHttpRequest {
                metadata: build_upload_metadata(
                    file_name.clone(),
                    mime_type.clone(),
                    size,
                    file_type,
                    manifest.local_upload_id.clone(),
                    user_id.clone(),
                ),
                desired_part_size: i64::try_from(options.chunk_size)
                    .map_err(|_| FlareError::general_error("invalid part size"))?,
                file_fingerprint: file_fingerprint.clone(),
                head_tail_sha256: head_tail_sha256.clone(),
                full_sha256: full_sha256.clone().unwrap_or_default(),
            };
            let body: HttpApiResponse<InitiateDirectUploadHttpResponse> = self
                .http
                .post_with_headers("/api/v1/medias/uploads/initiate", &req, &headers)
                .await?;
            let init = unwrap_api_response(body, "initiate direct upload")?;
            if !init.success {
                return Err(FlareError::localized(
                    ErrorCode::GeneralError,
                    init.error_message
                        .unwrap_or_else(|| "initiate direct upload failed".to_string()),
                ));
            }
            manifest.remote_upload_id = Some(init.upload_id.clone());
            manifest.file_id = Some(init.file_id.clone());
            manifest.storage_upload_id = init.storage_upload_id.clone();
            manifest.transport_kind = Some(match init.transport_kind {
                DirectUploadTransportKindHttp::SinglePut => DirectUploadTransportKindVo::SinglePut,
                DirectUploadTransportKindHttp::MultipartPut => {
                    DirectUploadTransportKindVo::MultipartPut
                }
            });
            manifest.bucket = Some(init.bucket.clone());
            manifest.object_key = Some(init.object_key.clone());
            manifest.upload_url = init.upload_url.clone();
            manifest.part_size = u32::try_from(init.part_size.max(1)).unwrap_or(u32::MAX);
            manifest.total_parts = init.total_parts.max(1);
            manifest.state = UploadManifestState::Uploading;
            manifest.updated_at_ms = now_ms();
            if let Some(store) = &self.upload_manifest_store {
                store.upsert_manifest(&manifest).await?;
                if manifest.transport_kind == Some(DirectUploadTransportKindVo::MultipartPut) {
                    let parts = build_upload_parts_from_manifest(&manifest);
                    store
                        .replace_parts(&manifest.local_upload_id, &parts)
                        .await?;
                }
            }
        }

        match manifest
            .transport_kind
            .clone()
            .unwrap_or(DirectUploadTransportKindVo::SinglePut)
        {
            DirectUploadTransportKindVo::SinglePut => {
                let upload_url = manifest.upload_url.clone().ok_or_else(|| {
                    FlareError::localized(
                        ErrorCode::GeneralError,
                        "single put upload_url missing in upload manifest",
                    )
                })?;
                emit_progress(
                    on_progress,
                    UploadProgress {
                        file_name: file_name.clone(),
                        upload_id: manifest.remote_upload_id.clone().unwrap_or_default(),
                        phase: UploadPhase::Uploading,
                        uploaded_bytes: 0,
                        total_bytes: size as u64,
                        chunk_index: Some(0),
                        total_chunks: Some(1),
                    },
                );
                let mut put_headers = HashMap::new();
                put_headers.insert("Content-Type".to_string(), mime_type.clone());
                let _ = self
                    .http
                    .put_file_full_url(&upload_url, path, size as u64, &put_headers)
                    .await?;
                emit_progress(
                    on_progress,
                    UploadProgress {
                        file_name: file_name.clone(),
                        upload_id: manifest.remote_upload_id.clone().unwrap_or_default(),
                        phase: UploadPhase::Completing,
                        uploaded_bytes: size as u64,
                        total_bytes: size as u64,
                        chunk_index: Some(0),
                        total_chunks: Some(1),
                    },
                );
            }
            DirectUploadTransportKindVo::MultipartPut => {
                let upload_id = manifest.remote_upload_id.clone().ok_or_else(|| {
                    FlareError::localized(ErrorCode::GeneralError, "remote_upload_id missing")
                })?;
                let headers = self.build_control_headers(&upload_id);
                let status_body: HttpApiResponse<GetDirectUploadStatusHttpResponse> = self
                    .http
                    .get_with_headers(
                        "/api/v1/medias/uploads/status",
                        Some(&HashMap::from([(
                            "upload_id".to_string(),
                            upload_id.clone(),
                        )])),
                        &headers,
                    )
                    .await?;
                let status = unwrap_api_response(status_body, "get direct upload status")?;

                let mut parts = if let Some(store) = &self.upload_manifest_store {
                    let existing = store.list_parts(&manifest.local_upload_id).await?;
                    if existing.is_empty() {
                        let generated = build_upload_parts_from_manifest(&manifest);
                        store
                            .replace_parts(&manifest.local_upload_id, &generated)
                            .await?;
                        generated
                    } else {
                        existing
                    }
                } else {
                    build_upload_parts_from_manifest(&manifest)
                };

                for server_part in status.uploaded_parts {
                    if let Some(part) = parts
                        .iter_mut()
                        .find(|part| part.part_number == server_part.part_number)
                    {
                        part.uploaded = true;
                        part.etag = Some(server_part.etag);
                    }
                }

                let missing_parts = parts
                    .iter()
                    .filter(|part| !part.uploaded)
                    .map(|part| part.part_number)
                    .collect::<Vec<_>>();

                if !missing_parts.is_empty() {
                    let presign_body: HttpApiResponse<PresignDirectUploadPartsHttpResponse> = self
                        .http
                        .post_with_headers(
                            "/api/v1/medias/uploads/presign-parts",
                            &PresignDirectUploadPartsHttpRequest {
                                upload_id: upload_id.clone(),
                                part_numbers: missing_parts.clone(),
                                expires_in: 3600,
                            },
                            &headers,
                        )
                        .await?;
                    let presigned =
                        unwrap_api_response(presign_body, "presign direct upload parts")?;
                    let presigned_map = presigned
                        .parts
                        .into_iter()
                        .map(|part| (part.part_number, part))
                        .collect::<HashMap<_, _>>();

                    let mut uploaded_bytes = parts
                        .iter()
                        .filter(|part| part.uploaded)
                        .map(|part| part.size)
                        .sum::<u64>();

                    let upload_path = Arc::new(path.to_path_buf());
                    let upload_jobs = parts
                        .iter()
                        .filter(|part| !part.uploaded)
                        .map(|part| {
                            let presigned_part = presigned_map
                                .get(&part.part_number)
                                .cloned()
                                .ok_or_else(|| {
                                    FlareError::general_error(
                                        "missing presigned url for upload part",
                                    )
                                })?;
                            Ok((part.clone(), presigned_part))
                        })
                        .collect::<Result<Vec<_>>>()?;
                    let mut upload_stream = futures_util::stream::iter(
                        upload_jobs.into_iter().map(|(part, presigned_part)| {
                            let http = self.http.clone();
                            let upload_path = Arc::clone(&upload_path);
                            async move {
                                let data =
                                    read_part_bytes(upload_path.as_path(), part.offset, part.size)
                                        .await?;
                                let sha256 = hex::encode(Sha256::digest(&data));
                                let headers_map = http
                                    .put_bytes_full_url(
                                        &presigned_part.upload_url,
                                        &data,
                                        &presigned_part.headers,
                                    )
                                    .await?;
                                let etag = headers_map
                                    .get("etag")
                                    .cloned()
                                    .or_else(|| headers_map.get("ETag").cloned())
                                    .ok_or_else(|| {
                                        FlareError::general_error(
                                            "object storage response missing ETag",
                                        )
                                    })?;
                                Ok::<UploadedDirectPart, FlareError>(UploadedDirectPart {
                                    part_number: part.part_number,
                                    size: part.size,
                                    sha256,
                                    etag,
                                })
                            }
                        }),
                    )
                    .buffer_unordered(MAX_CONCURRENT_DIRECT_UPLOAD_PARTS);

                    let mut uploaded_parts = Vec::new();
                    while let Some(result) = upload_stream.next().await {
                        let uploaded_part = result?;
                        uploaded_bytes = uploaded_bytes.saturating_add(uploaded_part.size);
                        emit_progress(
                            on_progress,
                            UploadProgress {
                                file_name: file_name.clone(),
                                upload_id: upload_id.clone(),
                                phase: UploadPhase::Uploading,
                                uploaded_bytes,
                                total_bytes: size as u64,
                                chunk_index: Some(uploaded_part.part_number - 1),
                                total_chunks: Some(manifest.total_parts),
                            },
                        );
                        uploaded_parts.push(uploaded_part);
                    }

                    if !uploaded_parts.is_empty() {
                        let commit_parts = uploaded_parts
                            .iter()
                            .map(|part| UploadedPartInfoHttp {
                                part_number: part.part_number,
                                etag: part.etag.clone(),
                                size: part.size as i64,
                                sha256: Some(part.sha256.clone()),
                            })
                            .collect::<Vec<_>>();
                        let commit_body: HttpApiResponse<CommitDirectUploadPartsHttpResponse> =
                            self.http
                                .post_with_headers(
                                    "/api/v1/medias/uploads/commit-parts",
                                    &CommitDirectUploadPartsHttpRequest {
                                        upload_id: upload_id.clone(),
                                        parts: commit_parts,
                                    },
                                    &headers,
                                )
                                .await?;
                        let _ = unwrap_api_response(commit_body, "commit direct upload parts")?;

                        let uploaded_map = uploaded_parts
                            .into_iter()
                            .map(|part| (part.part_number, part))
                            .collect::<HashMap<_, _>>();
                        for part in parts.iter_mut().filter(|part| !part.uploaded) {
                            if let Some(uploaded) = uploaded_map.get(&part.part_number) {
                                part.uploaded = true;
                                part.sha256 = uploaded.sha256.clone();
                                part.etag = Some(uploaded.etag.clone());
                            }
                        }
                    }
                }
                if let Some(store) = &self.upload_manifest_store {
                    store
                        .replace_parts(&manifest.local_upload_id, &parts)
                        .await?;
                }
            }
        }

        let upload_id = manifest.remote_upload_id.clone().ok_or_else(|| {
            FlareError::localized(
                ErrorCode::GeneralError,
                "remote_upload_id missing for complete",
            )
        })?;
        let headers = self.build_control_headers(&upload_id);
        let complete_body: HttpApiResponse<UploadFileHttpResponse> = self
            .http
            .post_with_headers(
                "/api/v1/medias/uploads/complete",
                &CompleteDirectUploadHttpRequest {
                    upload_id: upload_id.clone(),
                },
                &headers,
            )
            .await?;
        let data = unwrap_api_response(complete_body, "complete direct upload")?;
        if !data.success {
            return Err(FlareError::localized(
                ErrorCode::GeneralError,
                data.error_message
                    .unwrap_or_else(|| "complete direct upload failed".to_string()),
            ));
        }

        if let Some(store) = &self.upload_manifest_store {
            store.delete_manifest(&manifest.local_upload_id).await?;
        }

        emit_progress(
            on_progress,
            UploadProgress {
                file_name: file_name.clone(),
                upload_id,
                phase: UploadPhase::Finished,
                uploaded_bytes: size as u64,
                total_bytes: size as u64,
                chunk_index: None,
                total_chunks: Some(manifest.total_parts.max(1)),
            },
        );

        Ok(upload_file_to_uploaded_media(
            data, file_name, mime_type, size,
        ))
    }

    async fn upload_bytes_direct(
        &self,
        bytes: &[u8],
        file_name: String,
        mime_type: String,
        options: UploadOptions,
        on_progress: Option<&UploadProgressCallback>,
    ) -> Result<UploadedMedia> {
        if file_name.trim().is_empty() {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "upload_bytes requires file_name",
            ));
        }
        if mime_type.trim().is_empty() {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "upload_bytes requires mime_type",
            ));
        }
        let user_id = self.current_user_id.read().await.clone();
        let size = i64::try_from(bytes.len())
            .map_err(|_| FlareError::general_error("upload payload too large"))?;
        let file_type = infer_file_type(&mime_type);
        let (file_fingerprint, head_tail_sha256, full_sha256) = compute_bytes_fingerprints(bytes);
        let local_upload_id = random_upload_id("native-bytes");

        emit_progress(
            on_progress,
            UploadProgress {
                file_name: file_name.clone(),
                upload_id: local_upload_id.clone(),
                phase: UploadPhase::Preparing,
                uploaded_bytes: 0,
                total_bytes: bytes.len() as u64,
                chunk_index: None,
                total_chunks: None,
            },
        );

        let headers = self.build_control_headers(&local_upload_id);
        let body: HttpApiResponse<InitiateDirectUploadHttpResponse> = self
            .http
            .post_with_headers(
                "/api/v1/medias/uploads/initiate",
                &InitiateDirectUploadHttpRequest {
                    metadata: build_upload_metadata(
                        file_name.clone(),
                        mime_type.clone(),
                        size,
                        file_type,
                        local_upload_id.clone(),
                        user_id,
                    ),
                    desired_part_size: i64::try_from(options.chunk_size)
                        .map_err(|_| FlareError::general_error("invalid part size"))?,
                    file_fingerprint,
                    head_tail_sha256,
                    full_sha256: full_sha256.unwrap_or_default(),
                },
                &headers,
            )
            .await?;
        let init = unwrap_api_response(body, "initiate direct upload")?;
        if !init.success {
            return Err(FlareError::localized(
                ErrorCode::GeneralError,
                init.error_message
                    .unwrap_or_else(|| "initiate direct upload failed".to_string()),
            ));
        }

        let upload_id = init.upload_id.clone();
        match init.transport_kind {
            DirectUploadTransportKindHttp::SinglePut => {
                let upload_url = init.upload_url.clone().ok_or_else(|| {
                    FlareError::localized(ErrorCode::GeneralError, "single put upload_url missing")
                })?;
                emit_progress(
                    on_progress,
                    UploadProgress {
                        file_name: file_name.clone(),
                        upload_id: upload_id.clone(),
                        phase: UploadPhase::Uploading,
                        uploaded_bytes: 0,
                        total_bytes: bytes.len() as u64,
                        chunk_index: Some(0),
                        total_chunks: Some(1),
                    },
                );
                let mut put_headers = HashMap::new();
                put_headers.insert("Content-Type".to_string(), mime_type.clone());
                self.http
                    .put_bytes_full_url(&upload_url, bytes, &put_headers)
                    .await?;
            }
            DirectUploadTransportKindHttp::MultipartPut => {
                self.upload_multipart_bytes(
                    bytes,
                    &upload_id,
                    &file_name,
                    init.part_size.max(1) as u64,
                    init.total_parts.max(1),
                    on_progress,
                )
                .await?;
            }
        }

        emit_progress(
            on_progress,
            UploadProgress {
                file_name: file_name.clone(),
                upload_id: upload_id.clone(),
                phase: UploadPhase::Completing,
                uploaded_bytes: bytes.len() as u64,
                total_bytes: bytes.len() as u64,
                chunk_index: None,
                total_chunks: Some(init.total_parts.max(1)),
            },
        );

        let complete_body: HttpApiResponse<UploadFileHttpResponse> = self
            .http
            .post_with_headers(
                "/api/v1/medias/uploads/complete",
                &CompleteDirectUploadHttpRequest {
                    upload_id: upload_id.clone(),
                },
                &self.build_control_headers(&upload_id),
            )
            .await?;
        let data = unwrap_api_response(complete_body, "complete direct upload")?;
        if !data.success {
            return Err(FlareError::localized(
                ErrorCode::GeneralError,
                data.error_message
                    .unwrap_or_else(|| "complete direct upload failed".to_string()),
            ));
        }

        emit_progress(
            on_progress,
            UploadProgress {
                file_name: file_name.clone(),
                upload_id,
                phase: UploadPhase::Finished,
                uploaded_bytes: bytes.len() as u64,
                total_bytes: bytes.len() as u64,
                chunk_index: None,
                total_chunks: Some(init.total_parts.max(1)),
            },
        );

        Ok(upload_file_to_uploaded_media(
            data, file_name, mime_type, size,
        ))
    }

    async fn upload_multipart_bytes(
        &self,
        bytes: &[u8],
        upload_id: &str,
        file_name: &str,
        part_size: u64,
        total_parts: u32,
        on_progress: Option<&UploadProgressCallback>,
    ) -> Result<()> {
        let headers = self.build_control_headers(upload_id);
        let status_body: HttpApiResponse<GetDirectUploadStatusHttpResponse> = self
            .http
            .get_with_headers(
                "/api/v1/medias/uploads/status",
                Some(&HashMap::from([(
                    "upload_id".to_string(),
                    upload_id.to_string(),
                )])),
                &headers,
            )
            .await?;
        let status = unwrap_api_response(status_body, "get direct upload status")?;
        let mut parts = build_upload_parts(bytes.len() as u64, part_size, total_parts, upload_id);
        for server_part in status.uploaded_parts {
            if let Some(part) = parts
                .iter_mut()
                .find(|part| part.part_number == server_part.part_number)
            {
                part.uploaded = true;
                part.etag = Some(server_part.etag);
            }
        }

        let missing = parts
            .iter()
            .filter(|part| !part.uploaded)
            .map(|part| part.part_number)
            .collect::<Vec<_>>();
        if missing.is_empty() {
            return Ok(());
        }

        let presign_body: HttpApiResponse<PresignDirectUploadPartsHttpResponse> = self
            .http
            .post_with_headers(
                "/api/v1/medias/uploads/presign-parts",
                &PresignDirectUploadPartsHttpRequest {
                    upload_id: upload_id.to_string(),
                    part_numbers: missing,
                    expires_in: 3600,
                },
                &headers,
            )
            .await?;
        let presigned = unwrap_api_response(presign_body, "presign direct upload parts")?;
        let presigned_map = presigned
            .parts
            .into_iter()
            .map(|part| (part.part_number, part))
            .collect::<HashMap<_, _>>();

        let mut uploaded_bytes = parts
            .iter()
            .filter(|part| part.uploaded)
            .map(|part| part.size)
            .sum::<u64>();
        let upload_jobs = parts
            .iter()
            .filter(|part| !part.uploaded)
            .map(|part| {
                let presigned_part =
                    presigned_map
                        .get(&part.part_number)
                        .cloned()
                        .ok_or_else(|| {
                            FlareError::general_error("missing presigned url for upload part")
                        })?;
                Ok((part.clone(), presigned_part))
            })
            .collect::<Result<Vec<_>>>()?;
        let mut upload_stream =
            futures_util::stream::iter(upload_jobs.into_iter().map(|(part, presigned_part)| {
                let http = self.http.clone();
                let data = bytes[part.offset as usize..(part.offset + part.size) as usize].to_vec();
                async move {
                    let sha256 = hex::encode(Sha256::digest(&data));
                    let headers_map = http
                        .put_bytes_full_url(
                            &presigned_part.upload_url,
                            &data,
                            &presigned_part.headers,
                        )
                        .await?;
                    let etag = headers_map
                        .get("etag")
                        .cloned()
                        .or_else(|| headers_map.get("ETag").cloned())
                        .ok_or_else(|| {
                            FlareError::general_error("object storage response missing ETag")
                        })?;
                    Ok::<UploadedDirectPart, FlareError>(UploadedDirectPart {
                        part_number: part.part_number,
                        size: part.size,
                        sha256,
                        etag,
                    })
                }
            }))
            .buffer_unordered(MAX_CONCURRENT_DIRECT_UPLOAD_PARTS);

        let mut uploaded_parts = Vec::new();
        while let Some(result) = upload_stream.next().await {
            let uploaded_part = result?;
            uploaded_bytes = uploaded_bytes.saturating_add(uploaded_part.size);
            emit_progress(
                on_progress,
                UploadProgress {
                    file_name: file_name.to_string(),
                    upload_id: upload_id.to_string(),
                    phase: UploadPhase::Uploading,
                    uploaded_bytes,
                    total_bytes: bytes.len() as u64,
                    chunk_index: Some(uploaded_part.part_number - 1),
                    total_chunks: Some(total_parts),
                },
            );
            uploaded_parts.push(uploaded_part);
        }

        if uploaded_parts.is_empty() {
            return Ok(());
        }
        let commit_parts = uploaded_parts
            .iter()
            .map(|part| UploadedPartInfoHttp {
                part_number: part.part_number,
                etag: part.etag.clone(),
                size: part.size as i64,
                sha256: Some(part.sha256.clone()),
            })
            .collect::<Vec<_>>();
        let commit_body: HttpApiResponse<CommitDirectUploadPartsHttpResponse> = self
            .http
            .post_with_headers(
                "/api/v1/medias/uploads/commit-parts",
                &CommitDirectUploadPartsHttpRequest {
                    upload_id: upload_id.to_string(),
                    parts: commit_parts,
                },
                &headers,
            )
            .await?;
        let _ = unwrap_api_response(commit_body, "commit direct upload parts")?;
        Ok(())
    }

    fn build_control_headers(&self, trace_seed: &str) -> HashMap<String, String> {
        shared_build_control_headers(trace_seed)
    }

    fn new_manifest(&self, input: NewUploadManifest<'_>) -> MediaUploadManifestVo {
        let now = now_ms();
        MediaUploadManifestVo {
            local_upload_id: random_upload_id("direct"),
            remote_upload_id: None,
            file_id: None,
            storage_upload_id: None,
            tenant_id: String::new(),
            user_id: input.user_id.to_string(),
            source_kind: UploadSourceKind::StableFile,
            source_locator: input.source_locator.to_string(),
            file_name: input.file_name.to_string(),
            mime_type: input.mime_type.to_string(),
            file_size: input.file_size,
            part_size: 0,
            total_parts: 0,
            transport_kind: None,
            bucket: None,
            object_key: None,
            upload_url: None,
            file_fingerprint: input.file_fingerprint.to_string(),
            head_tail_sha256: input.head_tail_sha256.to_string(),
            full_sha256: input.full_sha256,
            state: UploadManifestState::Initiating,
            last_error_code: None,
            last_error_message: None,
            expires_at_ms: None,
            created_at_ms: now,
            updated_at_ms: now,
        }
    }
}

#[async_trait]
impl MediaProcessorPort for MediaService {
    async fn inspect(&self, source: &MediaSourceDescriptor) -> Result<MediaMetadata> {
        if let Some(metadata) = &source.metadata {
            return Ok(metadata.clone());
        }

        let local_path = local_path_from_media_source(source)?;
        let path = Path::new(&local_path);
        let file_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| FlareError::localized(ErrorCode::InvalidParameter, "invalid file name"))?
            .to_string();
        let metadata = tokio::fs::metadata(path)
            .await
            .map_err(|e| FlareError::general_error(format!("read file metadata failed: {e}")))?;

        Ok(MediaMetadata {
            mime_type: infer_mime_type(&file_name),
            file_name,
            size: metadata.len(),
            width: None,
            height: None,
            duration_ms: None,
            extra: HashMap::new(),
        })
    }

    async fn prepare_upload(
        &self,
        source: MediaSourceDescriptor,
        _options: Option<UploadOptions>,
    ) -> Result<ProcessedMedia> {
        let metadata = self.inspect(&source).await?;
        Ok(ProcessedMedia {
            source,
            metadata,
            payload: None,
        })
    }
}

#[async_trait]
impl MediaUploaderPort for MediaService {
    async fn upload(
        &self,
        media: ProcessedMedia,
        options: Option<UploadOptions>,
        progress: Option<UploadProgressSink>,
    ) -> Result<UploadedMedia> {
        if let Some(bytes) = media.payload {
            let progress = progress.map(|sink| Arc::new(sink) as UploadProgressCallback);
            return self
                .upload_bytes_with_progress(
                    bytes,
                    media.metadata.file_name,
                    media.metadata.mime_type,
                    options,
                    progress,
                )
                .await;
        }
        let local_path = local_path_from_media_source(&media.source)?;
        let progress = progress.map(|sink| Arc::new(sink) as UploadProgressCallback);
        self.upload_file_from_path_with_progress(Path::new(&local_path), options, progress)
            .await
    }
}

#[async_trait]
impl MediaServicePort for MediaService {
    async fn upload(
        &self,
        media: ProcessedMedia,
        options: Option<UploadOptions>,
        progress: Option<UploadProgressSink>,
    ) -> Result<UploadedMedia> {
        <Self as MediaUploaderPort>::upload(self, media, options, progress).await
    }

    async fn delete_file(&self, file_id: &str, hard_delete: bool) -> Result<bool> {
        MediaService::delete_file(self, file_id, hard_delete).await
    }

    async fn get_file_url(&self, file_id: &str, expires_in: i32) -> Result<MediaAccessUrl> {
        MediaService::get_file_url(self, file_id, expires_in).await
    }

    async fn get_temp_url_for_file_download(
        &self,
        file_id: &str,
        expires_in: i32,
    ) -> Result<MediaAccessUrl> {
        MediaService::get_temp_url_for_file_download(self, file_id, expires_in).await
    }

    async fn resolve_media_access(
        &self,
        file_id: &str,
        expires_in: i32,
    ) -> Result<MediaResolvedAccess> {
        MediaService::resolve_media_access(self, file_id, expires_in).await
    }

    async fn cache_remote_media(
        &self,
        file_id: &str,
        expires_in: i32,
    ) -> Result<MediaCacheEntryVo> {
        MediaService::cache_remote_media(self, file_id, expires_in).await
    }

    async fn media_cache_stats(&self) -> Result<crate::domain::MediaCacheStatsVo> {
        MediaService::media_cache_stats(self).await
    }

    async fn set_media_cache_max_bytes(&self, max_bytes: u64) -> Result<()> {
        MediaService::set_media_cache_max_bytes(self, max_bytes).await
    }

    async fn set_media_cache_root(&self, absolute_path: Option<&str>) -> Result<()> {
        MediaService::set_media_cache_root(self, absolute_path).await
    }

    async fn clear_media_cache(&self) -> Result<()> {
        MediaService::clear_media_cache(self).await
    }

    fn cancel_user_file_download(&self, download_key: &str) -> bool {
        MediaService::cancel_user_file_download(self, download_key)
    }

    async fn user_download_get_subfolder(&self) -> Result<String> {
        MediaService::user_download_get_subfolder(self).await
    }

    async fn user_download_set_subfolder(&self, name: &str) -> Result<()> {
        MediaService::user_download_set_subfolder(self, name).await
    }

    async fn user_download_get_saved_path(&self, download_key: &str) -> Result<Option<String>> {
        MediaService::user_download_get_saved_path(self, download_key).await
    }

    async fn user_download_delete_record(&self, download_key: &str) -> Result<()> {
        MediaService::user_download_delete_record(self, download_key).await
    }

    async fn download_file_to_user_downloads_folder(
        &self,
        request: UserFileDownloadRequest,
    ) -> Result<String> {
        MediaService::download_file_to_user_downloads_folder(self, request).await
    }
}

fn local_path_from_media_source(source: &MediaSourceDescriptor) -> Result<String> {
    match &source.kind {
        MediaSourceKind::Path => Ok(source.locator.clone()),
        MediaSourceKind::Uri => Ok(source
            .locator
            .strip_prefix("file://")
            .unwrap_or(&source.locator)
            .to_string()),
        other => Err(FlareError::localized(
            ErrorCode::OperationNotSupported,
            format!("media source kind is not supported by native MediaService: {other:?}"),
        )),
    }
}

async fn compute_file_fingerprints(path: &Path) -> Result<(String, String, Option<String>)> {
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|e| FlareError::general_error(format!("read file metadata failed: {e}")))?;
    let file_size = metadata.len();

    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| FlareError::general_error(format!("open file failed: {e}")))?;

    let mut head = vec![0_u8; usize::try_from(file_size.min(1024 * 1024)).unwrap_or(0)];
    if !head.is_empty() {
        file.read_exact(&mut head)
            .await
            .map_err(|e| FlareError::general_error(format!("read file head failed: {e}")))?;
    }

    let tail_len = usize::try_from(file_size.min(1024 * 1024)).unwrap_or(0);
    let mut tail = vec![0_u8; tail_len];
    if tail_len > 0 {
        file.seek(SeekFrom::Start(file_size.saturating_sub(tail_len as u64)))
            .await
            .map_err(|e| FlareError::general_error(format!("seek file tail failed: {e}")))?;
        file.read_exact(&mut tail)
            .await
            .map_err(|e| FlareError::general_error(format!("read file tail failed: {e}")))?;
    }

    let head_hash = hex::encode(Sha256::digest(&head));
    let tail_hash = hex::encode(Sha256::digest(&tail));
    let head_tail_sha256 = hex::encode(Sha256::digest(
        format!("{head_hash}:{tail_hash}:{file_size}").as_bytes(),
    ));
    let fingerprint = hex::encode(Sha256::digest(
        format!("{file_size}:{head_hash}:{tail_hash}").as_bytes(),
    ));

    Ok((fingerprint, head_tail_sha256, None))
}

async fn read_part_bytes(path: &Path, offset: u64, size: u64) -> Result<Vec<u8>> {
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| FlareError::general_error(format!("open file failed: {e}")))?;
    file.seek(SeekFrom::Start(offset))
        .await
        .map_err(|e| FlareError::general_error(format!("seek file failed: {e}")))?;
    let mut data =
        vec![0_u8; usize::try_from(size).map_err(|_| FlareError::general_error("part too large"))?];
    file.read_exact(&mut data)
        .await
        .map_err(|e| FlareError::general_error(format!("read file part failed: {e}")))?;
    Ok(data)
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn emit_progress(on_progress: Option<&UploadProgressCallback>, progress: UploadProgress) {
    if let Some(cb) = on_progress {
        cb(progress);
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl MediaService {
    /// 取消进行中的「下载到用户下载目录」任务（与 `download_key` 对应）。
    pub fn cancel_user_file_download(&self, download_key: &str) -> bool {
        let k = download_key.trim();
        if k.is_empty() {
            return false;
        }
        let Ok(g) = self.download_cancel_flags.lock() else {
            return false;
        };
        g.get(k).map(|f| f.store(false, Ordering::SeqCst)).is_some()
    }

    pub async fn user_download_get_subfolder(&self) -> Result<String> {
        let store = self.user_file_download_store.as_ref().ok_or_else(|| {
            FlareError::localized(
                ErrorCode::ConfigurationError,
                "user file download store is not configured",
            )
        })?;
        store.get_download_subfolder().await
    }

    pub async fn user_download_set_subfolder(&self, name: &str) -> Result<()> {
        let store = self.user_file_download_store.as_ref().ok_or_else(|| {
            FlareError::localized(
                ErrorCode::ConfigurationError,
                "user file download store is not configured",
            )
        })?;
        store.set_download_subfolder(name).await
    }

    pub async fn user_download_get_saved_path(&self, download_key: &str) -> Result<Option<String>> {
        let store = self.user_file_download_store.as_ref().ok_or_else(|| {
            FlareError::localized(
                ErrorCode::ConfigurationError,
                "user file download store is not configured",
            )
        })?;
        store.get_saved_path(download_key).await
    }

    pub async fn user_download_delete_record(&self, download_key: &str) -> Result<()> {
        let store = self.user_file_download_store.as_ref().ok_or_else(|| {
            FlareError::localized(
                ErrorCode::ConfigurationError,
                "user file download store is not configured",
            )
        })?;
        store.delete_download_record(download_key).await
    }

    /// 将文件保存到「系统下载目录 / 可配置子目录」，并写入 SQLite `user_file_download`。
    ///
    /// 来源优先级：`source_path` → `source_http_url` → `remote_file_id`（经网关取临时直链）。
    pub async fn download_file_to_user_downloads_folder(
        &self,
        request: UserFileDownloadRequest,
    ) -> Result<String> {
        let UserFileDownloadRequest {
            download_key,
            display_file_name,
            source_path,
            source_http_url,
            remote_file_id,
            expires_in,
            on_progress,
        } = request;
        let key = download_key.trim().to_string();
        if key.is_empty() {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "download_file_to_user_downloads_folder: empty download_key",
            ));
        }

        let store = self.user_file_download_store.as_ref().ok_or_else(|| {
            FlareError::localized(
                ErrorCode::ConfigurationError,
                "user file download store is not configured",
            )
        })?;

        let run_flag = Arc::new(AtomicBool::new(true));
        {
            let mut m = self.download_cancel_flags.lock().map_err(|_| {
                FlareError::localized(ErrorCode::InternalError, "download cancel map lock failed")
            })?;
            m.insert(key.clone(), run_flag.clone());
        }

        let result = async {
            let sub = store.get_download_subfolder().await?;
            let base = dirs::download_dir().ok_or_else(|| {
                FlareError::localized(ErrorCode::ConfigurationError, "cannot resolve download dir")
            })?;
            let dir = base.join(sub.trim());
            tokio::fs::create_dir_all(&dir).await.map_err(|e| {
                FlareError::localized(
                    ErrorCode::GeneralError,
                    format!("create download subdir failed: {e}"),
                )
            })?;

            let safe_name = sanitize_user_download_file_name(&display_file_name);
            let dest_path = checked_user_download_destination(&dir, &safe_name)?;

            let sp = source_path
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty());
            let su = source_http_url
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty());
            let rf = remote_file_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty());

            let out_path = if let Some(p) = sp {
                Self::user_download_copy_from_path(p, &dest_path, &run_flag, on_progress.as_ref())
                    .await?
            } else if let Some(u) = su {
                if !(u.starts_with("http://") || u.starts_with("https://")) {
                    return Err(FlareError::localized(
                        ErrorCode::InvalidParameter,
                        "source_http_url must be http(s)",
                    ));
                }
                Self::user_download_stream_http(
                    &self.http,
                    u,
                    &dest_path,
                    &run_flag,
                    on_progress.as_ref(),
                )
                .await?
            } else if let Some(fid) = rf {
                let access = self.get_temp_url_for_file_download(fid, expires_in).await?;
                let url = pick_download_url(&access);
                if url.is_empty() {
                    return Err(FlareError::localized(
                        ErrorCode::GeneralError,
                        "empty download url from gateway",
                    ));
                }
                Self::user_download_stream_http(
                    &self.http,
                    url,
                    &dest_path,
                    &run_flag,
                    on_progress.as_ref(),
                )
                .await?
            } else {
                return Err(FlareError::localized(
                    ErrorCode::InvalidParameter,
                    "provide source_path, source_http_url, or remote_file_id",
                ));
            };

            let path_str = out_path.to_string_lossy().into_owned();
            store
                .save_download_record(&key, &path_str, &display_file_name)
                .await?;
            Ok(path_str)
        }
        .await;

        if let Ok(mut m) = self.download_cancel_flags.lock() {
            m.remove(&key);
        }

        result
    }

    async fn user_download_copy_from_path(
        src_raw: &str,
        dest: &Path,
        run_flag: &AtomicBool,
        on_progress: Option<&FileDownloadProgressCallback>,
    ) -> Result<PathBuf> {
        let src = resolve_user_download_source_path(src_raw);
        if !src.is_file() {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "source file does not exist",
            ));
        }
        let total = tokio::fs::metadata(&src)
            .await
            .map_err(|e| FlareError::general_error(format!("metadata: {e}")))?
            .len();
        enforce_user_download_byte_budget(Some(total), 0, 0)?;
        emit_file_download_progress(on_progress, 0, Some(total));
        let mut reader = tokio::fs::File::open(&src)
            .await
            .map_err(|e| FlareError::general_error(format!("open source: {e}")))?;
        let mut writer = tokio::fs::File::create(dest)
            .await
            .map_err(|e| FlareError::general_error(format!("create dest: {e}")))?;
        let mut buf = vec![0u8; 256 * 1024];
        let mut downloaded: u64 = 0;
        loop {
            if !run_flag.load(Ordering::Relaxed) {
                drop(writer);
                let _ = tokio::fs::remove_file(dest).await;
                return Err(FlareError::general_error("下载已取消"));
            }
            let n = reader
                .read(&mut buf)
                .await
                .map_err(|e| FlareError::general_error(format!("read: {e}")))?;
            if n == 0 {
                break;
            }
            enforce_user_download_byte_budget(Some(total), downloaded, n as u64)?;
            writer
                .write_all(&buf[..n])
                .await
                .map_err(|e| FlareError::general_error(format!("write: {e}")))?;
            downloaded += n as u64;
            emit_file_download_progress(on_progress, downloaded, Some(total));
        }
        writer.flush().await.ok();
        emit_file_download_progress(on_progress, downloaded, Some(total));
        Ok(dest.to_path_buf())
    }

    async fn user_download_stream_http(
        http: &HttpClient,
        url: &str,
        dest: &Path,
        run_flag: &AtomicBool,
        on_progress: Option<&FileDownloadProgressCallback>,
    ) -> Result<PathBuf> {
        let resp = http.get_response_direct_url(url).await?;
        let total = resp.content_length();
        enforce_user_download_byte_budget(total, 0, 0)?;
        emit_file_download_progress(on_progress, 0, total);
        let mut stream = resp.bytes_stream();
        let mut file = tokio::fs::File::create(dest)
            .await
            .map_err(|e| FlareError::general_error(format!("create dest: {e}")))?;
        let mut downloaded: u64 = 0;
        while let Some(item) = stream.next().await {
            if !run_flag.load(Ordering::Relaxed) {
                drop(file);
                let _ = tokio::fs::remove_file(dest).await;
                return Err(FlareError::general_error("下载已取消"));
            }
            let chunk = item.map_err(|e| FlareError::system(format!("http chunk: {e}")))?;
            enforce_user_download_byte_budget(total, downloaded, chunk.len() as u64)?;
            file.write_all(&chunk)
                .await
                .map_err(|e| FlareError::general_error(format!("write: {e}")))?;
            downloaded += chunk.len() as u64;
            emit_file_download_progress(on_progress, downloaded, total);
        }
        file.flush()
            .await
            .map_err(|e| FlareError::general_error(format!("flush: {e}")))?;
        Ok(dest.to_path_buf())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn emit_file_download_progress(
    on: Option<&FileDownloadProgressCallback>,
    downloaded: u64,
    total: Option<u64>,
) {
    if let Some(cb) = on {
        cb(FileDownloadProgress { downloaded, total });
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn sanitize_user_download_file_name(name: &str) -> String {
    let base = name
        .trim()
        .rsplit(['/', '\\'])
        .find(|segment| !segment.trim().is_empty())
        .map(str::trim)
        .unwrap_or("");
    if base.is_empty() {
        return "download".to_string();
    }
    let sanitized = base
        .chars()
        .map(|ch| {
            if ch.is_control() || matches!(ch, '?' | '%' | '*' | ':' | '|' | '"' | '<' | '>') {
                '_'
            } else {
                ch
            }
        })
        .take(200)
        .collect::<String>();
    let sanitized = sanitized.trim_matches([' ', '.']).to_string();
    if sanitized.is_empty() || sanitized == "." || sanitized == ".." {
        return "download".to_string();
    }
    if is_windows_reserved_file_name(&sanitized) {
        return format!("{sanitized}_");
    }
    sanitized
}

#[cfg(not(target_arch = "wasm32"))]
fn is_windows_reserved_file_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).to_ascii_lowercase();
    matches!(
        stem.as_str(),
        "con"
            | "prn"
            | "aux"
            | "nul"
            | "com1"
            | "com2"
            | "com3"
            | "com4"
            | "com5"
            | "com6"
            | "com7"
            | "com8"
            | "com9"
            | "lpt1"
            | "lpt2"
            | "lpt3"
            | "lpt4"
            | "lpt5"
            | "lpt6"
            | "lpt7"
            | "lpt8"
            | "lpt9"
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn resolve_user_download_source_path(raw: &str) -> std::path::PathBuf {
    let t = raw.trim();
    if t.to_lowercase().starts_with("file:")
        && let Ok(u) = url::Url::parse(t)
        && let Ok(pb) = u.to_file_path()
    {
        return pb;
    }
    PathBuf::from(t)
}

#[cfg(not(target_arch = "wasm32"))]
fn checked_user_download_destination(dir: &Path, file_name: &str) -> Result<PathBuf> {
    let dest = unique_user_download_destination(dir, file_name);
    ensure_user_download_destination_in_dir(dir, dest)
}

#[cfg(not(target_arch = "wasm32"))]
fn ensure_user_download_destination_in_dir(dir: &Path, dest: PathBuf) -> Result<PathBuf> {
    if dest.starts_with(dir) && dest.parent().is_some_and(|parent| parent == dir) {
        Ok(dest)
    } else {
        Err(FlareError::localized(
            ErrorCode::InvalidParameter,
            "download destination escapes user download directory",
        ))
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn unique_user_download_destination(dir: &Path, file_name: &str) -> PathBuf {
    let dest = dir.join(file_name);
    if !dest.exists() {
        return dest;
    }
    let stem = Path::new(file_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    let ext = Path::new(file_name)
        .extension()
        .and_then(|s| s.to_str())
        .map(|e| format!(".{e}"))
        .unwrap_or_default();
    for i in 1..10_000 {
        let candidate = dir.join(format!("{stem} ({i}){ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    dir.join(format!("{stem}_{t}{ext}"))
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod user_download_policy_tests {
    use super::*;

    #[test]
    fn rejects_content_length_over_user_download_budget() {
        let err = enforce_user_download_byte_budget(Some(MAX_USER_DOWNLOAD_BYTES + 1), 0, 0)
            .expect_err("content length beyond budget must fail");

        assert_eq!(err.code(), Some(ErrorCode::ResourceExhausted));
    }

    #[test]
    fn rejects_chunked_download_when_accumulated_bytes_exceed_budget() {
        let err = enforce_user_download_byte_budget(None, MAX_USER_DOWNLOAD_BYTES, 1)
            .expect_err("chunked response beyond budget must fail");

        assert_eq!(err.code(), Some(ErrorCode::ResourceExhausted));
    }

    #[test]
    fn sanitizes_user_download_file_name_to_safe_basename() {
        assert_eq!(
            sanitize_user_download_file_name("../../etc/passwd"),
            "passwd"
        );
        assert_eq!(
            sanitize_user_download_file_name(r"C:\tmp\payload?.txt"),
            "payload_.txt"
        );
        assert_eq!(sanitize_user_download_file_name(".."), "download");
        assert_eq!(sanitize_user_download_file_name(".hidden."), "hidden");
        assert_eq!(sanitize_user_download_file_name("CON.txt"), "CON.txt_");
        assert_eq!(
            sanitize_user_download_file_name("bad\u{0000}\u{001f}name.txt"),
            "bad__name.txt"
        );
    }

    #[test]
    fn rejects_download_destination_outside_user_download_dir() {
        let dir = Path::new("/tmp/flare-downloads");

        assert!(
            ensure_user_download_destination_in_dir(dir, dir.join("safe.txt")).is_ok(),
            "direct child path should be accepted"
        );
        assert!(
            ensure_user_download_destination_in_dir(dir, PathBuf::from("/tmp/evil.txt")).is_err(),
            "sibling path must be rejected"
        );
        assert!(
            ensure_user_download_destination_in_dir(dir, dir.join("nested").join("evil.txt"))
                .is_err(),
            "nested path must be rejected because downloads are saved as basenames"
        );
    }
}

/// 选择用于下载/展示的 HTTP 地址。
///
/// `flare-media` 对私有对象：`url` 为 S3 预签名链接，`cdn_url` 常为 `cdn_base + object_path`（无签名）。
/// 因此 `url` 是权威访问地址，`cdn_url` 只作为后备展示 hint。
fn pick_download_url(access: &MediaAccessUrl) -> &str {
    let u = access.url.trim();
    if !u.is_empty() {
        return u;
    }
    access.cdn_url.as_deref().unwrap_or("").trim()
}

#[cfg(test)]
mod media_access_url_selection_tests {
    use super::*;

    #[test]
    fn pick_download_url_prefers_core_media_url_over_cdn_hint() {
        let access = MediaAccessUrl {
            url: "http://127.0.0.1:29000/flare-media/private.png?X-Amz-Signature=ok".to_string(),
            cdn_url: Some("http://127.0.0.1:29000/flare-media/private.png".to_string()),
        };

        assert_eq!(
            pick_download_url(&access),
            "http://127.0.0.1:29000/flare-media/private.png?X-Amz-Signature=ok"
        );
    }

    #[test]
    fn pick_download_url_uses_cdn_hint_only_when_core_url_is_absent() {
        let access = MediaAccessUrl {
            url: " ".to_string(),
            cdn_url: Some("http://127.0.0.1:29000/flare-media/public.png".to_string()),
        };

        assert_eq!(
            pick_download_url(&access),
            "http://127.0.0.1:29000/flare-media/public.png"
        );
    }
}

fn infer_mime_from_url_or_octet_stream(url: &str, bytes: &[u8]) -> String {
    let path = url.split('?').next().unwrap_or(url);
    if let Some(idx) = path.rfind('/') {
        let name = &path[idx + 1..];
        if name.contains('.') {
            return infer_mime_type(name);
        }
    }
    if bytes.len() >= 3 && bytes[..3] == [0xFF, 0xD8, 0xFF] {
        return "image/jpeg".to_string();
    }
    if bytes.len() >= 8 && bytes[..8] == [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A] {
        return "image/png".to_string();
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return "image/webp".to_string();
    }
    "application/octet-stream".to_string()
}

fn validate_path_mime_prefix(path: &Path, expected_prefix: &str) -> Result<()> {
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| FlareError::localized(ErrorCode::InvalidParameter, "invalid file name"))?;
    let mime = infer_mime_type(file_name);
    if !mime.starts_with(expected_prefix) {
        return Err(FlareError::localized(
            ErrorCode::InvalidParameter,
            format!("expected {expected_prefix} mime type, got {mime}"),
        ));
    }
    Ok(())
}

fn infer_mime_type(file_name: &str) -> String {
    let lower = file_name.to_ascii_lowercase();
    if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg".to_string()
    } else if lower.ends_with(".png") {
        "image/png".to_string()
    } else if lower.ends_with(".gif") {
        "image/gif".to_string()
    } else if lower.ends_with(".webp") {
        "image/webp".to_string()
    } else if lower.ends_with(".mp4") {
        "video/mp4".to_string()
    } else if lower.ends_with(".webm") {
        "video/webm".to_string()
    } else if lower.ends_with(".mp3") {
        "audio/mpeg".to_string()
    } else if lower.ends_with(".wav") {
        "audio/wav".to_string()
    } else if lower.ends_with(".aac") {
        "audio/aac".to_string()
    } else if lower.ends_with(".pdf") {
        "application/pdf".to_string()
    } else {
        "application/octet-stream".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_upload_parts_respects_part_boundaries() {
        let manifest = MediaUploadManifestVo {
            local_upload_id: "local-upload-1".to_string(),
            remote_upload_id: None,
            file_id: None,
            storage_upload_id: None,
            tenant_id: String::new(),
            user_id: "u1".to_string(),
            source_kind: UploadSourceKind::StableFile,
            source_locator: "/tmp/demo.bin".to_string(),
            file_name: "demo.bin".to_string(),
            mime_type: "application/octet-stream".to_string(),
            file_size: 10,
            part_size: 4,
            total_parts: 3,
            transport_kind: Some(DirectUploadTransportKindVo::MultipartPut),
            bucket: None,
            object_key: None,
            upload_url: None,
            file_fingerprint: "fp".to_string(),
            head_tail_sha256: "ht".to_string(),
            full_sha256: None,
            state: UploadManifestState::Uploading,
            last_error_code: None,
            last_error_message: None,
            expires_at_ms: None,
            created_at_ms: 0,
            updated_at_ms: 0,
        };

        let parts = build_upload_parts_from_manifest(&manifest);
        assert_eq!(parts.len(), 3);
        assert_eq!(
            (parts[0].part_number, parts[0].offset, parts[0].size),
            (1, 0, 4)
        );
        assert_eq!(
            (parts[1].part_number, parts[1].offset, parts[1].size),
            (2, 4, 4)
        );
        assert_eq!(
            (parts[2].part_number, parts[2].offset, parts[2].size),
            (3, 8, 2)
        );
    }

    #[test]
    fn infer_mime_and_file_type_cover_common_media() {
        assert_eq!(infer_mime_type("photo.png"), "image/png");
        assert_eq!(infer_mime_type("clip.mp4"), "video/mp4");
        assert_eq!(infer_mime_type("voice.mp3"), "audio/mpeg");
        assert_eq!(infer_mime_type("report.pdf"), "application/pdf");
    }
}
