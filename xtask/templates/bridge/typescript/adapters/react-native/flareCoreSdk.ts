// GENERATED. Do not edit by hand.
import type { NativeBridge } from '../../contract/bridge_contract';
import type { FlareImClient } from '../../api';
import { DefaultFlareImClient } from '../../adapter/defaultFlareImClient';
import { FfiNativeBridge } from '../../bridge/ffiNativeBridge';

export abstract class FlareCoreSdk {
  static createClient(bridge?: NativeBridge): FlareImClient {
    return new DefaultFlareImClient(bridge ?? new FfiNativeBridge());
  }

  static createClientWithBridge(bridge: NativeBridge): FlareImClient {
    return new DefaultFlareImClient(bridge);
  }
}
