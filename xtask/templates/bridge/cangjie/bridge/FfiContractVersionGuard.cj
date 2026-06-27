package flare_core_harmony_cangjie_sdk.bridge

import flare_core_harmony_cangjie_sdk.contract.*

// GENERATED. Do not edit by hand.
public class FfiContractVersionGuard {
    public static func unavailable(operation!: String): Unit {
        throw Exception(
            "FlareSdkException(code=contract.version_unavailable, operation=${SdkOperations.diagnosticsFfiContractVersion}, expected=${SdkContract.ffiContractVersion}, transport=ffi, requestedOperation=${operation}, message=Native binding contract version is required because FfiNativeBridge is not linked to libflare_im_core_sdk_ffi yet.)"
        )
    }
}
