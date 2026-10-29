// GENERATED. Do not edit by hand.

/**
 * The structured part of an SDK failure, as every binding hands it over
 * (`bindings/contract/errors.json`). Hosts read these fields; `message` is for people.
 */
export interface FlareSdkErrorDetails {
  /** Localized key or reason, e.g. `sdk.plugin.disabled`. */
  reason?: string;
  /** Metadata Core and the SDK attached, e.g. a plugin rejection's `machineReason`. */
  params?: Readonly<Record<string, string>>;
  /** Whether Core judges the failure worth retrying. */
  retryable?: boolean;
}

/** Stable TypeScript error surfaced by platform bridges. */
export class FlareSdkException extends Error {
  /** The core `ErrorCode` wire name of an SDK failure (`PERMISSION_DENIED`), else the bridge's own code. */
  readonly code: string;
  readonly operation?: string;
  readonly details?: Record<string, string>;
  readonly reason?: string;
  readonly params?: Readonly<Record<string, string>>;
  readonly retryable: boolean;

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
    this.params = sdk.params;
    this.retryable = sdk.retryable ?? false;
  }

  /**
   * The exception for an error a binding threw or rejected with: the wasm runtime's `Error`
   * or a Tauri command's rejection object, both carrying `code`, `reason`, `params` and
   * `retryable`. Null when `error` carries no code.
   */
  static fromBindingError(error: unknown, operation?: string): FlareSdkException | null {
    if (error instanceof FlareSdkException) return error;
    if (!error || typeof error !== 'object') return null;
    const value = error as Record<string, unknown>;
    if (typeof value.code !== 'string' || !value.code) return null;
    const params = stringRecord(value.params);
    return new FlareSdkException(
      value.code,
      typeof value.message === 'string' ? value.message : value.code,
      typeof value.operation === 'string' ? value.operation : operation,
      undefined,
      {
        reason: typeof value.reason === 'string' ? value.reason : undefined,
        params,
        retryable: value.retryable === true,
      },
    );
  }
}

function stringRecord(value: unknown): Readonly<Record<string, string>> | undefined {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return undefined;
  const entries = Object.entries(value as Record<string, unknown>).filter(
    (entry): entry is [string, string] => typeof entry[1] === 'string',
  );
  return entries.length ? Object.fromEntries(entries) : undefined;
}
