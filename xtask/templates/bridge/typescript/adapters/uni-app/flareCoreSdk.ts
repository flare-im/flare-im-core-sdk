// GENERATED. Do not edit by hand.
import type { NativeBridge } from '../../contract/bridge_contract';
import type { FlareImClient } from '../../api';
import { DefaultFlareImClient } from '../../adapter/defaultFlareImClient';
import { FfiNativeBridge } from '../../bridge/ffiNativeBridge';
import { WasmNativeBridge, type WasmBridgeOptions } from '../../bridge/wasmNativeBridge';

function isUniAppNativeShell(): boolean {
  if (typeof globalThis === 'undefined') {
    return false;
  }
  const uni = (globalThis as { uni?: { getSystemInfoSync?: () => { uniPlatform?: string } } }).uni;
  const platform = uni?.getSystemInfoSync?.().uniPlatform;
  return platform !== undefined && platform !== 'web' && platform !== 'h5';
}

export abstract class FlareCoreSdk {
  /** H5 uses wasm; App uses native module over bindings/c. */
  static createClient(options?: WasmBridgeOptions): FlareImClient {
    const bridge: NativeBridge = isUniAppNativeShell()
      ? new FfiNativeBridge()
      : new WasmNativeBridge(options);
    return new DefaultFlareImClient(bridge);
  }

  static createClientWithBridge(bridge: NativeBridge): FlareImClient {
    return new DefaultFlareImClient(bridge);
  }
}
