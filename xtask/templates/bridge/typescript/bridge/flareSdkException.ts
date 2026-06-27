// GENERATED. Do not edit by hand.

/** Stable TypeScript error surfaced by platform bridges. */
export class FlareSdkException extends Error {
  readonly code: string;
  readonly operation?: string;
  readonly details?: Record<string, string>;

  constructor(
    code: string,
    message: string,
    operation?: string,
    details?: Record<string, string>,
  ) {
    super(message);
    this.name = 'FlareSdkException';
    this.code = code;
    this.operation = operation;
    this.details = details;
  }
}
