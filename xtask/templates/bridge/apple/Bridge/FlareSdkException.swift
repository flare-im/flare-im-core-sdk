import Foundation

/// Stable Swift error surfaced by the Apple SDK bridge/adapter.
///
/// An SDK failure carries the binding's structured error (`bindings/contract/errors.json`):
/// `code` is the core error code (`PERMISSION_DENIED`), `reason` the localized key
/// (`sdk.plugin.disabled`), `machineReason` Core's or the plugin's code (`PLUGIN_DISABLED`,
/// `CALL_FULL`), `httpStatus` and `remoteCode` Core's answer. Read these, never `message`.
public struct FlareSdkException: Error, LocalizedError, Sendable {
    public let code: String
    public let message: String
    public let operation: String?
    public let details: [String: String]
    public let reason: String?
    public let retryable: Bool
    public let httpStatus: Int?
    public let remoteCode: Int?
    public let machineReason: String?

    public init(
        code: String,
        message: String,
        operation: String? = nil,
        details: [String: String] = [:],
        reason: String? = nil,
        retryable: Bool = false,
        httpStatus: Int? = nil,
        remoteCode: Int? = nil,
        machineReason: String? = nil
    ) {
        self.code = code
        self.message = message
        self.operation = operation
        self.details = details
        self.reason = reason
        self.retryable = retryable
        self.httpStatus = httpStatus
        self.remoteCode = remoteCode
        self.machineReason = machineReason
    }

    /// The error of a failed native call: the structured error the SDK wrote in `details_json`,
    /// else the C ABI code alone (`native_error_<code>`).
    public static func fromNative(
        nativeCode: Int32,
        message: String,
        operation: String?,
        detailsJson: String
    ) -> FlareSdkException {
        let structured = detailsJson.data(using: .utf8)
            .flatMap { try? JSONSerialization.jsonObject(with: $0) as? [String: Any] } ?? [:]
        let code = (structured["code"] as? String).flatMap { $0.isEmpty ? nil : $0 }
        return FlareSdkException(
            code: code ?? "native_error_\(nativeCode)",
            message: message,
            operation: operation,
            reason: structured["reason"] as? String,
            retryable: structured["retryable"] as? Bool ?? false,
            httpStatus: (structured["httpStatus"] as? NSNumber)?.intValue,
            remoteCode: (structured["remoteCode"] as? NSNumber)?.intValue,
            machineReason: structured["machineReason"] as? String
        )
    }

    public var errorDescription: String? {
        if let operation {
            return "FlareSdkException(code=\(code), operation=\(operation), message=\(message))"
        }
        return "FlareSdkException(code=\(code), message=\(message))"
    }
}
