package flare_core_harmony_cangjie_sdk.bridge

import flare_core_harmony_cangjie_sdk.contract.*

// GENERATED. Do not edit by hand.
// Thin Cangjie bridge over the Flare Core C ABI. Replace with real FFI dispatch.
public class FfiNativeBridge <: NativeBridge {
    public func invoke(descriptor!: NativeCallDescriptor, requestJson!: String): String {
        FfiContractVersionGuard.unavailable(descriptor.operation)
        throw Exception(
            "FlareSdkException(code=bridge.not_implemented, operation=${descriptor.operation}, cApi=${descriptor.cApi}, transport=${descriptor.transport}, returnMode=${descriptor.returnMode}, requestEncoding=${descriptor.requestEncoding}, responseEncoding=${descriptor.responseEncoding}, callback=${descriptor.callback}, handlePolicy=${descriptor.handlePolicy}, message=FfiNativeBridge is not linked to libflare_im_core_sdk_ffi yet.)"
        )
    }
}
