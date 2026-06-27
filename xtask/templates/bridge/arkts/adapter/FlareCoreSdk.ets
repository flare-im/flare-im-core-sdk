// GENERATED. Do not edit by hand.
import { NativeBridge } from '../contract/BridgeContract';
import type { FlareImClient } from '../api/client';
import { FfiNativeBridge } from '../bridge/FfiNativeBridge';
import { DefaultFlareImClient } from './DefaultFlareImClient';

/** Public entry for HarmonyOS apps. */
export class FlareCoreSdk {
  static createClient(bridge?: NativeBridge): FlareImClient {
    return new DefaultFlareImClient(bridge ?? new FfiNativeBridge());
  }

  static createClientWithBridge(bridge: NativeBridge): FlareImClient {
    return new DefaultFlareImClient(bridge);
  }
}
