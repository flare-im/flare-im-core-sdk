import Foundation

/// GENERATED. Do not edit by hand.
public enum FlareCoreSdk {
    /// Creates a client backed by the default FFI bridge.
    public static func createClient(libraryPath: String? = nil) throws -> any FlareImClientProtocol {
        try createClientWithBridge(FfiNativeBridge(libraryPath: libraryPath))
    }

    /// Creates a client from a custom bridge (tests, host IPC, mock).
    public static func createClientWithBridge(_ bridge: any NativeBridgeProtocol) -> any FlareImClientProtocol {
        DefaultFlareImClient(bridge: bridge)
    }
}
