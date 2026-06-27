// GENERATED. Do not edit by hand.

/** Stable ArkTS error surfaced by the HarmonyOS SDK bridge/adapter. */
export class FlareSdkException extends Error {
  readonly code: string;
  readonly operation?: string;
  readonly details?: Record<string, string>;

  constructor(code: string, message: string, operation?: string, details?: Record<string, string>) {
    super(message);
    this.name = 'FlareSdkException';
    this.code = code;
    this.operation = operation;
    this.details = details;
  }
}
