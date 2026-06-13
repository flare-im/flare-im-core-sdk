//! Shared async JSON dispatch helpers for generated C entrypoints.

use std::ffi::{c_char, c_void};
use std::future::Future;
use std::pin::Pin;

use crate::abi;
use crate::executor::{CallbackContext, execute_async, execute_async_unit, return_error};
use crate::helpers::{c_str_to_string, parse_json, to_json_string};
use crate::registry::SdkInstance;
use crate::registry::require_instance;
use crate::types::{FlareHandle, FlareResultCallback};
use flare_im_core_sdk_bindings_runtime::{BindingResponse, InvokeSession, invoke_api_id};
use std::sync::Arc;

type DispatchFut = Pin<Box<dyn Future<Output = flare_im_core_sdk::Result<BindingResponse>> + Send>>;

pub(crate) fn binding_response_to_json(response: BindingResponse) -> Result<String, i32> {
    if response.is_unit {
        Ok(String::new())
    } else {
        to_json_string(&response.payload)
    }
}

impl InvokeSession for SdkInstance {
    fn client(&self) -> flare_im_core_sdk::client::IMClient {
        self.client.clone()
    }

    fn message_api(
        &self,
    ) -> impl Future<Output = flare_im_core_sdk::Result<flare_im_core_sdk::client::api::MessageApi>> + Send
    {
        async move { self.message_api().await }
    }

    fn message_build_api(
        &self,
    ) -> impl Future<
        Output = flare_im_core_sdk::Result<
            std::sync::Arc<flare_im_core_sdk::client::api::MessageBuildApi>,
        >,
    > + Send {
        async move { self.message_build_api().await }
    }

    fn conversation_api(
        &self,
    ) -> impl Future<
        Output = flare_im_core_sdk::Result<flare_im_core_sdk::client::api::ConversationApi>,
    > + Send {
        async move { self.conversation_api().await }
    }

    fn media_api(
        &self,
    ) -> impl Future<
        Output = flare_im_core_sdk::Result<
            std::sync::Arc<flare_im_core_sdk::client::api::MediaApi>,
        >,
    > + Send {
        async move { self.media_api().await }
    }

    fn capability_api(
        &self,
    ) -> impl Future<
        Output = flare_im_core_sdk::Result<
            std::sync::Arc<flare_im_core_sdk::client::api::CapabilityApi>,
        >,
    > + Send {
        async move { self.capability_api().await }
    }

    fn after_disconnect(&self) -> impl Future<Output = ()> + Send {
        async move {
            self.im_session.clear().await;
        }
    }
}

pub(crate) fn json_dispatch_entry(
    handle: FlareHandle,
    op: *const c_char,
    params_json: *const c_char,
    context: *mut c_void,
    callback: FlareResultCallback,
    run: impl FnOnce(Arc<SdkInstance>, String, serde_json::Value) -> DispatchFut + Send + 'static,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };
        let operation = match c_str_to_string(op) {
            Ok(s) => s,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid op");
                return code;
            }
        };
        let params = match parse_json(params_json) {
            Ok(p) => p,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid params JSON");
                return code;
            }
        };
        let ctx = CallbackContext::new(context, callback);
        execute_async(
            instance.clone(),
            ctx,
            async move { run(instance, operation, params).await },
            binding_response_to_json,
        );
        0
    })
}

pub(crate) fn message_build_dispatch_entry(
    handle: FlareHandle,
    request_json: *const c_char,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };
        let request = match parse_json(request_json) {
            Ok(p) => p,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid request JSON");
                return code;
            }
        };
        let ctx = CallbackContext::new(context, callback);
        execute_async(
            instance.clone(),
            ctx,
            async move {
                let api = instance.message_build_api().await?;
                flare_im_core_sdk_bindings_runtime::message_build::dispatch_message_build(
                    &api, request,
                )
                .await
            },
            binding_response_to_json,
        );
        0
    })
}

pub(crate) fn typed_invoke_unit(
    instance: Arc<SdkInstance>,
    ctx: CallbackContext,
    api_id: &str,
    params: serde_json::Value,
) {
    let inst = instance.clone();
    let api_id = api_id.to_string();
    execute_async_unit(instance, ctx, async move {
        let _ = invoke_api_id(inst.as_ref(), &api_id, params).await?;
        Ok(())
    });
}

pub(crate) fn typed_invoke_json(
    instance: Arc<SdkInstance>,
    ctx: CallbackContext,
    api_id: &str,
    params: serde_json::Value,
) {
    let inst = instance.clone();
    let api_id = api_id.to_string();
    execute_async(
        instance,
        ctx,
        async move { invoke_api_id(inst.as_ref(), &api_id, params).await },
        binding_response_to_json,
    );
}

pub(crate) fn typed_invoke_send_ack(
    instance: Arc<SdkInstance>,
    ctx: CallbackContext,
    api_id: &str,
    params: serde_json::Value,
) {
    typed_invoke_json(instance, ctx, api_id, params);
}

pub(crate) fn invoke_entry(
    handle: FlareHandle,
    api_id: *const c_char,
    params_json: *const c_char,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };
        let api_id = match c_str_to_string(api_id) {
            Ok(s) => s,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid api_id");
                return code;
            }
        };
        let params = match parse_json(params_json) {
            Ok(p) => p,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid params JSON");
                return code;
            }
        };
        let ctx = CallbackContext::new(context, callback);
        execute_async(
            instance.clone(),
            ctx,
            async move { invoke_api_id(instance.as_ref(), &api_id, params).await },
            binding_response_to_json,
        );
        0
    })
}
