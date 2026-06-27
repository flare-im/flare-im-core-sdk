import Foundation

enum FfiContractVersionGuard {
    static func validate(_ value: String) throws {
        let actual = value.trimmingCharacters(in: .whitespacesAndNewlines)
        if actual.isEmpty {
            throw FlareSdkException(
                code: "contract.version_unavailable",
                message: "Native binding contract version is required.",
                operation: SdkOperations.diagnosticsFfiContractVersion,
                details: [
                    "expected": SdkContract.ffiContractVersion,
                    "transport": "ffi",
                ]
            )
        }
        if actual != SdkContract.ffiContractVersion {
            throw FlareSdkException(
                code: "contract.version_mismatch",
                message: "Native binding contract version \(actual) does not match SDK \(SdkContract.ffiContractVersion).",
                operation: SdkOperations.diagnosticsFfiContractVersion,
                details: [
                    "expected": SdkContract.ffiContractVersion,
                    "actual": actual,
                    "transport": "ffi",
                ]
            )
        }
    }
}
