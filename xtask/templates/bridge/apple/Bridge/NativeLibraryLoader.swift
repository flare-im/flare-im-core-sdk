import Foundation

private let libraryBaseName = "flare_im_core_sdk_ffi"

/// Loads the Flare Core C ABI library for Apple platforms.
public enum NativeLibraryLoader {
    public static func load(libraryPath: String? = nil) throws -> UnsafeMutableRawPointer {
        if let libraryPath, !libraryPath.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            guard let handle = dlopen(libraryPath, RTLD_NOW) else {
                throw loaderError(path: libraryPath)
            }
            return handle
        }

        #if os(iOS)
        if let handle = dlopen(nil, RTLD_NOW) {
            return handle
        }
        #endif

        let candidates: [String] = {
            #if os(macOS)
            return [
                "lib\(libraryBaseName).dylib",
                "@rpath/lib\(libraryBaseName).dylib",
                "Frameworks/lib\(libraryBaseName).dylib",
            ]
            #else
            return ["lib\(libraryBaseName).dylib"]
            #endif
        }()

        var lastError: String?
        for candidate in candidates {
            if let handle = dlopen(candidate, RTLD_NOW) {
                return handle
            }
            lastError = String(cString: dlerror())
        }

        throw FlareSdkException(
            code: "native_library_load_failed",
            message: "Unable to load lib\(libraryBaseName). Build and bundle the FFI artifact first.",
            details: [
                "candidates": candidates.joined(separator: ","),
                "last_error": lastError ?? "unknown",
            ]
        )
    }

    private static func loaderError(path: String) -> FlareSdkException {
        FlareSdkException(
            code: "native_library_load_failed",
            message: String(cString: dlerror()),
            details: ["path": path]
        )
    }
}
