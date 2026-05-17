//! 网络层（Network）— IO 与逻辑分离，仅负责连接与帧收发。

pub mod http;
pub mod socket;

pub use http::{
    AbortDirectUploadHttpRequest, CommitDirectUploadPartsHttpRequest,
    CommitDirectUploadPartsHttpResponse, CompleteDirectUploadHttpRequest, DeleteFileHttpRequest,
    DeleteFileHttpResponse, DirectUploadTransportKindHttp, FileInfoHttpResponse,
    GetDirectUploadStatusHttpResponse, GetFileUrlHttpRequest, GetFileUrlHttpResponse,
    HttpApiResponse, HttpClient, HttpRequestContext, InitiateDirectUploadHttpRequest,
    InitiateDirectUploadHttpResponse, PresignDirectUploadPartsHttpRequest,
    PresignDirectUploadPartsHttpResponse, PresignedUploadPartHttp, UploadFileHttpResponse,
    UploadFileMetadataHttp, UploadedPartInfoHttp, unwrap_api_response,
};
pub use socket::{SocketHandler, SocketTransport};
