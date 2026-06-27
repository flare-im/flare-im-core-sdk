package flare_core_harmony_cangjie_sdk

import flare_core_harmony_cangjie_sdk.contract.*
import flare_core_harmony_cangjie_sdk.adapter.*
import flare_core_harmony_cangjie_sdk.bridge.*

// GENERATED. Do not edit by hand.
public class FlareCoreSdk {
    public static func createClient(bridge: NativeBridge = FfiNativeBridge()): FlareImClient {
        return DefaultFlareImClient(bridge)
    }

    public static func createClientWithBridge(bridge: NativeBridge): FlareImClient {
        return DefaultFlareImClient(bridge)
    }
}
