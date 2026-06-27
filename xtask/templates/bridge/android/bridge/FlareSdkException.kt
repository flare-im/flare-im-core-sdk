package com.flare.im.bridge

/** Stable Android error surfaced by the JNI/FFI bridge/adapter. */
class FlareSdkException(
    val code: String,
    override val message: String,
    val operation: String? = null,
    val details: Map<String, String> = emptyMap(),
) : RuntimeException(message)
