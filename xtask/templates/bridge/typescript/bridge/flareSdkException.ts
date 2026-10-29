// GENERATED. Do not edit by hand.

/**
 * The structured part of an SDK failure, as every binding hands it over
 * (`bindings/contract/errors.json`). Hosts read these fields; `message` is for people.
 */
export interface FlareSdkErrorDetails {
  /** Localized key or reason, e.g. `sdk.plugin.disabled`. */
  reason?: string;
  /** Whether Core judges the failure worth retrying. */
  retryable?: boolean;
  /** The HTTP status Core answered (100..=599). */
  httpStatus?: number;
  /** Core's numeric error code. */
  remoteCode?: number;
  /** Core's or the plugin's code, e.g. `PLUGIN_DISABLED`, `CALL_FULL`. */
  machineReason?: string;
}

/** Stable TypeScript error surfaced by platform bridges. */
export class FlareSdkException extends Error {
  /** The core `ErrorCode` wire name of an SDK failure (`PERMISSION_DENIED`), else the bridge's own code. */
  readonly code: string;
  readonly operation?: string;
  readonly details?: Record<string, string>;
  readonly reason?: string;
  readonly retryable: boolean;
  readonly httpStatus?: number;
  readonly remoteCode?: number;
  readonly machineReason?: string;

  constructor(
    code: string,
    message: string,
    operation?: string,
    details?: Record<string, string>,
    sdk: FlareSdkErrorDetails = {},
  ) {
    super(message);
    this.name = 'FlareSdkException';
    this.code = code;
    this.operation = operation;
    this.details = details;
    this.reason = sdk.reason;
    this.retryable = sdk.retryable ?? false;
    this.httpStatus = sdk.httpStatus;
    this.remoteCode = sdk.remoteCode;
    this.machineReason = sdk.machineReason;
  }

  /**
   * The exception for an error a binding threw or rejected with: the wasm runtime's `Error`
   * or a Tauri command's rejection object, both carrying the structured error. Null when
   * `error` carries no code.
   */
  static fromBindingError(error: unknown, operation?: string): FlareSdkException | null {
    if (error instanceof FlareSdkException) return error;
    if (!error || typeof error !== 'object') return null;
    const value = error as Record<string, unknown>;
    if (typeof value.code !== 'string' || !value.code) return null;
    return new FlareSdkException(
      value.code,
      typeof value.message === 'string' ? value.message : value.code,
      typeof value.operation === 'string' ? value.operation : operation,
      undefined,
      {
        reason: typeof value.reason === 'string' ? value.reason : undefined,
        retryable: value.retryable === true,
        httpStatus: typeof value.httpStatus === 'number' ? value.httpStatus : undefined,
        remoteCode: typeof value.remoteCode === 'number' ? value.remoteCode : undefined,
        machineReason: typeof value.machineReason === 'string' ? value.machineReason : undefined,
      },
    );
  }
}
