package com.flare.im

import com.flare.im.adapter.DefaultFlareImClient
import com.flare.im.api.FlareImClient
import com.flare.im.bridge.JniNativeBridge
import com.flare.im.contract.NativeBridge

/** Public entry for Android apps. */
object FlareCoreSdk {
    fun createClient(bridge: NativeBridge = JniNativeBridge()): FlareImClient =
        DefaultFlareImClient(bridge)

    fun createClientWithBridge(bridge: NativeBridge): FlareImClient =
        DefaultFlareImClient(bridge)
}
