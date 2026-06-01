//! 媒体上传与下载：直传分片、网关取链、本地缓存、附件下载到用户目录并落库。

use std::collections::HashMap;
use std::path::Path;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(not(target_arch = "wasm32"))]
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
#[cfg(not(target_arch = "wasm32"))]
use tokio::io::AsyncWriteExt;
use tokio::io::{AsyncReadExt, AsyncSeekExt, SeekFrom};
use tokio::sync::RwLock;

use crate::application::sdk_callbacks::{
    FileDownloadProgress, FileDownloadProgressCallback, UploadPhase, UploadProgress,
    UploadProgressCallback, UserFileDownloadRequest,
};
use crate::domain::{
    DirectUploadTransportKindVo, MediaCacheAdmin, MediaCacheEntryVo, MediaCacheStore,
    MediaUploadManifestVo, MediaUploadPartVo, UploadManifestState, UploadManifestStore,
    UploadSourceKind, UserFileDownloadStore,
};
use crate::error::{ErrorCode, FlareError, Result};
use crate::model::{MediaAccessUrl, MediaResolvedAccess, UploadOptions, UploadedMedia};
use crate::transport::{
    CommitDirectUploadPartsHttpRequest, CommitDirectUploadPartsHttpResponse,
    CompleteDirectUploadHttpRequest, DeleteFileHttpRequest, DeleteFileHttpResponse,
    DirectUploadTransportKindHttp, GetDirectUploadStatusHttpResponse, GetFileUrlHttpRequest,
    GetFileUrlHttpResponse, HttpApiResponse, HttpClient, InitiateDirectUploadHttpRequest,
    InitiateDirectUploadHttpResponse, PresignDirectUploadPartsHttpRequest,
    PresignDirectUploadPartsHttpResponse, UploadFileHttpResponse, UploadFileMetadataHttp,
    UploadedPartInfoHttp, unwrap_api_response,
};

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
                    let parts = build_upload_parts(&manifest);
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
                let payload = tokio::fs::read(path)
                    .await
                    .map_err(|e| FlareError::general_error(format!("read file failed: {e}")))?;
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
                    .put_bytes_full_url(&upload_url, &payload, &put_headers)
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
                        let generated = build_upload_parts(&manifest);
                        store
                            .replace_parts(&manifest.local_upload_id, &generated)
                            .await?;
                        generated
                    } else {
                        existing
                    }
                } else {
                    build_upload_parts(&manifest)
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

                    for part in parts.iter_mut().filter(|part| !part.uploaded) {
                        let presigned_part =
                            presigned_map.get(&part.part_number).ok_or_else(|| {
                                FlareError::general_error("missing presigned url for upload part")
                            })?;
                        emit_progress(
                            on_progress,
                            UploadProgress {
                                file_name: file_name.clone(),
                                upload_id: upload_id.clone(),
                                phase: UploadPhase::Uploading,
                                uploaded_bytes,
                                total_bytes: size as u64,
                                chunk_index: Some(part.part_number - 1),
                                total_chunks: Some(manifest.total_parts),
                            },
                        );
                        let data = read_part_bytes(path, part.offset, part.size).await?;
                        part.sha256 = hex::encode(Sha256::digest(&data));
                        let headers_map = self
                            .http
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
                        let commit_body: HttpApiResponse<CommitDirectUploadPartsHttpResponse> =
                            self.http
                                .post_with_headers(
                                    "/api/v1/medias/uploads/commit-parts",
                                    &CommitDirectUploadPartsHttpRequest {
                                        upload_id: upload_id.clone(),
                                        parts: vec![UploadedPartInfoHttp {
                                            part_number: part.part_number,
                                            etag: etag.clone(),
                                            size: part.size as i64,
                                            sha256: Some(part.sha256.clone()),
                                        }],
                                    },
                                    &headers,
                                )
                                .await?;
                        let _ = unwrap_api_response(commit_body, "commit direct upload part")?;
                        part.uploaded = true;
                        part.etag = Some(etag);
                        uploaded_bytes = uploaded_bytes.saturating_add(part.size);
                        if let Some(store) = &self.upload_manifest_store {
                            store.upsert_part(part).await?;
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

    fn build_control_headers(&self, trace_seed: &str) -> HashMap<String, String> {
        HashMap::from([(
            "x-trace-id".to_string(),
            format!("sdk-upload-{trace_seed}-{}", rand::random::<u32>()),
        )])
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

    let mut full_file = tokio::fs::File::open(path)
        .await
        .map_err(|e| FlareError::general_error(format!("open file failed: {e}")))?;
    let mut full_hasher = Sha256::new();
    let mut buffer = vec![0_u8; 256 * 1024];
    loop {
        let read = full_file
            .read(&mut buffer)
            .await
            .map_err(|e| FlareError::general_error(format!("stream read failed: {e}")))?;
        if read == 0 {
            break;
        }
        full_hasher.update(&buffer[..read]);
    }
    let full_sha256 = hex::encode(full_hasher.finalize());

    let head_hash = hex::encode(Sha256::digest(&head));
    let tail_hash = hex::encode(Sha256::digest(&tail));
    let head_tail_sha256 = hex::encode(Sha256::digest(
        format!("{head_hash}:{tail_hash}:{file_size}").as_bytes(),
    ));
    let fingerprint = hex::encode(Sha256::digest(
        format!("{file_size}:{head_hash}:{tail_hash}").as_bytes(),
    ));

    Ok((fingerprint, head_tail_sha256, Some(full_sha256)))
}

fn build_upload_parts(manifest: &MediaUploadManifestVo) -> Vec<MediaUploadPartVo> {
    let total_parts = manifest.total_parts.max(1);
    let part_size = u64::from(manifest.part_size.max(1));
    let mut parts = Vec::with_capacity(total_parts as usize);
    for idx in 0..total_parts {
        let part_number = idx + 1;
        let offset = u64::from(idx) * part_size;
        let remaining = manifest.file_size.saturating_sub(offset);
        let size = remaining.min(part_size);
        parts.push(MediaUploadPartVo {
            local_upload_id: manifest.local_upload_id.clone(),
            part_number,
            offset,
            size,
            sha256: String::new(),
            etag: None,
            uploaded: false,
        });
    }
    parts
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

fn random_upload_id(prefix: &str) -> String {
    format!(
        "{prefix}-{}-{}",
        chrono::Utc::now().timestamp_millis(),
        rand::random::<u32>()
    )
}

fn upload_file_to_uploaded_media(
    data: UploadFileHttpResponse,
    fallback_file_name: String,
    fallback_mime_type: String,
    fallback_size: i64,
) -> UploadedMedia {
    let (resolved_name, resolved_mime, resolved_size) = if let Some(info) = data.info {
        (info.file_name, info.mime_type, info.size)
    } else {
        (fallback_file_name, fallback_mime_type, fallback_size)
    };
    UploadedMedia {
        file_id: data.file_id,
        file_name: resolved_name,
        mime_type: resolved_mime,
        size: resolved_size,
        url: data.url,
        cdn_url: data.cdn_url,
    }
}

fn build_upload_metadata(
    file_name: String,
    mime_type: String,
    file_size: i64,
    file_type: i32,
    upload_id: String,
    user_id: String,
) -> UploadFileMetadataHttp {
    UploadFileMetadataHttp {
        file_name,
        mime_type,
        file_size,
        file_type,
        upload_id: upload_id.clone(),
        metadata: HashMap::new(),
        user_id,
        trace_id: upload_id,
        namespace: "im.message".to_string(),
        business_tag: "chat_attachment".to_string(),
        bucket: String::new(),
        object_key: String::new(),
        labels: HashMap::new(),
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
            std::fs::create_dir_all(&dir).map_err(|e| {
                FlareError::localized(
                    ErrorCode::GeneralError,
                    format!("create download subdir failed: {e}"),
                )
            })?;

            let safe_name = sanitize_user_download_file_name(&display_file_name);
            let dest_path = unique_user_download_destination(&dir, &safe_name);

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
    let base = name.trim();
    if base.is_empty() {
        return "download".to_string();
    }
    base.replace(['/', '\\', '?', '%', '*', ':', '|', '"', '<', '>'], "_")
        .chars()
        .take(200)
        .collect()
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

/// 选择用于下载/展示的 HTTP 地址。
///
/// `flare-media` 对私有对象：`url` 为 S3 预签名链接，`cdn_url` 常为 `cdn_base + object_path`（无签名）。
/// 旧逻辑优先 `cdn_url` 会导致浏览器 GET 未授权路径返回 **403**。若 `url` 明显为预签名，必须优先使用。
fn is_likely_aws_presigned_get_url(url: &str) -> bool {
    let u = url.trim();
    u.contains("X-Amz-Algorithm=")
        || u.contains("X-Amz-Signature=")
        || u.contains("AWSAccessKeyId=")
}

fn pick_download_url(access: &MediaAccessUrl) -> &str {
    let u = access.url.trim();
    let c = access.cdn_url.as_deref().unwrap_or("").trim();
    if !u.is_empty() && is_likely_aws_presigned_get_url(u) {
        return u;
    }
    if !c.is_empty() {
        return c;
    }
    u
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

fn infer_file_type(mime: &str) -> i32 {
    if mime.starts_with("image/") {
        1
    } else if mime.starts_with("video/") {
        2
    } else if mime.starts_with("audio/") {
        3
    } else if mime == "application/pdf"
        || mime.starts_with("application/")
        || mime.starts_with("text/")
    {
        4
    } else {
        5
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

        let parts = build_upload_parts(&manifest);
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
        assert_eq!(infer_file_type("image/png"), 1);
        assert_eq!(infer_file_type("video/mp4"), 2);
        assert_eq!(infer_file_type("audio/mpeg"), 3);
        assert_eq!(infer_file_type("application/pdf"), 4);
        assert_eq!(infer_file_type("application/octet-stream"), 4);
    }
}
