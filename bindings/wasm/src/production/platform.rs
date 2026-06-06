//! WASM platform helpers shared with `flare-core::common::platform`.

use flare_core::common::error::FlareError;
use flare_core::common::platform::{
    AES256_KEY_LEN, clear_runtime_encryption_key, format_now_rfc3339, has_runtime_encryption_key,
    parse_encryption_key_hex, parse_encryption_key_utf8, register_aes256_encryption,
    runtime_instance_id, set_runtime_encryption_key, wall_clock_ms,
};
use wasm_bindgen::prelude::*;

/// Demo key matching `flare_chat_server` when no runtime key is injected.
const DEMO_ENCRYPTION_KEY: &[u8; AES256_KEY_LEN] = b"01234567890123456789012345678901";

fn map_err(err: FlareError) -> JsValue {
    JsValue::from_str(&err.to_string())
}

#[wasm_bindgen(js_name = flareEncryptionKeyLen)]
pub fn flare_encryption_key_len() -> u32 {
    AES256_KEY_LEN as u32
}

#[wasm_bindgen(js_name = flareSetEncryptionKey)]
pub fn flare_set_encryption_key(key: String) -> Result<(), JsValue> {
    let bytes = parse_encryption_key_utf8(&key).map_err(map_err)?;
    set_runtime_encryption_key(bytes).map_err(map_err)?;
    register_aes256_encryption(Some(DEMO_ENCRYPTION_KEY)).map_err(map_err)
}

#[wasm_bindgen(js_name = flareSetEncryptionKeyHex)]
pub fn flare_set_encryption_key_hex(hex: String) -> Result<(), JsValue> {
    let bytes = parse_encryption_key_hex(&hex).map_err(map_err)?;
    set_runtime_encryption_key(bytes).map_err(map_err)?;
    register_aes256_encryption(Some(DEMO_ENCRYPTION_KEY)).map_err(map_err)
}

#[wasm_bindgen(js_name = flareClearEncryptionKey)]
pub fn flare_clear_encryption_key() {
    clear_runtime_encryption_key();
}

#[wasm_bindgen(js_name = flareHasEncryptionKey)]
pub fn flare_has_encryption_key() -> bool {
    has_runtime_encryption_key()
}

#[wasm_bindgen(js_name = flareWallClockMs)]
pub fn flare_wall_clock_ms() -> u64 {
    wall_clock_ms()
}

#[wasm_bindgen(js_name = flareNowRfc3339)]
pub fn flare_now_rfc3339() -> String {
    format_now_rfc3339()
}

#[wasm_bindgen(js_name = flareRuntimeId)]
pub fn flare_runtime_id() -> String {
    runtime_instance_id()
}
