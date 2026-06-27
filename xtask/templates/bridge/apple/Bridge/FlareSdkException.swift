import Foundation

/// Stable Swift error surfaced by the Apple SDK bridge/adapter.
public struct FlareSdkException: Error, LocalizedError, Sendable {
    public let code: String
    public let message: String
    public let operation: String?
    public let details: [String: String]

    public init(code: String, message: String, operation: String? = nil, details: [String: String] = [:]) {
        self.code = code
        self.message = message
        self.operation = operation
        self.details = details
    }

    public var errorDescription: String? {
        if let operation {
            return "FlareSdkException(code=\(code), operation=\(operation), message=\(message))"
        }
        return "FlareSdkException(code=\(code), message=\(message))"
    }
}
