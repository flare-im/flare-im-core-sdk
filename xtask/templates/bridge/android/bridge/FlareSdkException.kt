package com.flare.im.bridge

import org.json.JSONObject

/**
 * Stable Android error surfaced by the JNI/FFI bridge/adapter.
 *
 * An SDK failure carries the binding's structured error (`bindings/contract/errors.json`):
 * [code] is the core error code (`PERMISSION_DENIED`), [reason] the localized key
 * (`sdk.plugin.disabled`), [machineReason] Core's or the plugin's code (`PLUGIN_DISABLED`,
 * `CALL_FULL`), [httpStatus] and [remoteCode] Core's answer, [refusedMessageIds] the messages
 * the operation refused. Read these, never [message].
 */
class FlareSdkException(
    val code: String,
    override val message: String,
    val operation: String? = null,
    val details: Map<String, String> = emptyMap(),
    val reason: String? = null,
    val retryable: Boolean = false,
    val httpStatus: Int? = null,
    val remoteCode: Int? = null,
    val machineReason: String? = null,
    val refusedMessageIds: List<String>? = null,
) : RuntimeException(message) {
    companion object {
        /**
         * The error of a failed native call: the structured error the SDK wrote in
         * `details_json`, else the C ABI code alone (`native.<code>`).
         */
        fun fromNative(
            nativeCode: Int,
            message: String,
            operation: String?,
            detailsJson: String?,
            details: Map<String, String> = emptyMap(),
        ): FlareSdkException {
            val structured = detailsJson
                ?.takeIf { it.isNotBlank() }
                ?.let { runCatching { JSONObject(it) }.getOrNull() }
            fun string(name: String): String? =
                structured?.takeIf { it.has(name) }?.optString(name)?.takeIf { it.isNotEmpty() }
            fun int(name: String): Int? =
                structured?.takeIf { it.has(name) }?.opt(name)?.let { it as? Number }?.toInt()
            fun strings(name: String): List<String>? {
                val array = structured?.optJSONArray(name)?.takeIf { it.length() > 0 } ?: return null
                return (0 until array.length()).map { array.opt(it) as? String ?: return null }
            }
            return FlareSdkException(
                code = string("code") ?: "native.$nativeCode",
                message = message,
                operation = operation,
                details = details,
                reason = string("reason"),
                retryable = structured?.optBoolean("retryable", false) ?: false,
                httpStatus = int("httpStatus"),
                remoteCode = int("remoteCode"),
                machineReason = string("machineReason"),
                refusedMessageIds = strings("refusedMessageIds"),
            )
        }
    }
}
