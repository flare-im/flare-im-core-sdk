// GENERATED. Do not edit by hand.
import { ffiContractVersion, SdkOperations } from '../contract/SdkContract';
import { FlareSdkException } from './FlareSdkException';

export function assertFfiContractVersion(value: string): void {
  const actual = value.trim();
  if (actual.length === 0) {
    throw new FlareSdkException(
      'contract.version_unavailable',
      'Native binding contract version is required.',
      SdkOperations.diagnosticsFfiContractVersion,
      {
        expected: ffiContractVersion,
        transport: 'ffi',
      },
    );
  }
  if (actual !== ffiContractVersion) {
    throw new FlareSdkException(
      'contract.version_mismatch',
      `Native binding contract version ${actual} does not match SDK ${ffiContractVersion}.`,
      SdkOperations.diagnosticsFfiContractVersion,
      {
        expected: ffiContractVersion,
        actual,
        transport: 'ffi',
      },
    );
  }
}
