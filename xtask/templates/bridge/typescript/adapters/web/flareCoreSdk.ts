// GENERATED. Do not edit by hand.
import type { FlareImClient } from '../../api';
import { DefaultFlareImClient } from '../../adapter/defaultFlareImClient';
import { WasmNativeBridge, type WasmBridgeOptions } from '../../bridge/wasmNativeBridge';

export abstract class FlareCoreSdk {
  static createClient(options?: WasmBridgeOptions): FlareImClient {
    return new DefaultFlareImClient(new WasmNativeBridge(options));
  }
}
