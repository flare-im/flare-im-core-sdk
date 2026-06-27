package com.flare.im.bridge

import com.flare.im.contract.SdkContract
import com.flare.im.contract.SdkOperations

internal object FfiContractVersionGuard {
    fun validate(value: String) {
        val actual = value.trim()
        if (actual.isEmpty()) {
            throw FlareSdkException(
                code = "contract.version_unavailable",
                message = "Native binding contract version is required.",
                operation = SdkOperations.DIAGNOSTICS_FFI_CONTRACT_VERSION,
                details = mapOf(
                    "expected" to SdkContract.FFI_CONTRACT_VERSION,
                    "transport" to "ffi",
                ),
            )
        }
        if (actual != SdkContract.FFI_CONTRACT_VERSION) {
            throw FlareSdkException(
                code = "contract.version_mismatch",
                message = "Native binding contract version $actual does not match SDK ${SdkContract.FFI_CONTRACT_VERSION}.",
                operation = SdkOperations.DIAGNOSTICS_FFI_CONTRACT_VERSION,
                details = mapOf(
                    "expected" to SdkContract.FFI_CONTRACT_VERSION,
                    "actual" to actual,
                    "transport" to "ffi",
                ),
            )
        }
    }
}
