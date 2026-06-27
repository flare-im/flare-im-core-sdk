import { ffiContractVersion } from '../contract/sdk_contract';
import { FlareSdkException } from './flareSdkException';

const CONTRACT_VERSION_OPERATION = 'diagnostics.ffi_contract_version';

function versionFromNative(value: unknown): string | undefined {
  if (typeof value === 'string') {
    return value.trim();
  }
  if (value && typeof value === 'object') {
    const version = (value as Record<string, unknown>).version;
    return typeof version === 'string' ? version.trim() : undefined;
  }
  return undefined;
}

export function assertBindingContractVersion(value: unknown, transport: string): void {
  const actual = versionFromNative(value);
  if (!actual) {
    throw new FlareSdkException(
      'contract.version_unavailable',
      'Native binding contract version is required',
      CONTRACT_VERSION_OPERATION,
      {
        expected: ffiContractVersion,
        transport,
      },
    );
  }
  if (actual !== ffiContractVersion) {
    throw new FlareSdkException(
      'contract.version_mismatch',
      `Native binding contract version ${actual} does not match SDK ${ffiContractVersion}`,
      CONTRACT_VERSION_OPERATION,
      {
        expected: ffiContractVersion,
        actual,
        transport,
      },
    );
  }
}
