// GENERATED. Do not edit by hand.
import type { NativeBridge } from '../../contract/bridge_contract';
import type { FlareImClient } from '../../api';
import { DefaultFlareImClient } from '../../adapter/defaultFlareImClient';
import { DefaultEventsApi } from '../../adapter/module/DefaultEventsApi';
import { TauriNativeBridge } from './tauriNativeBridge';

export abstract class FlareCoreSdk {
  static createClient(bridge?: NativeBridge): FlareImClient {
    const nativeBridge = bridge ?? new TauriNativeBridge();
    const client = new DefaultFlareImClient(nativeBridge);
    if (nativeBridge instanceof TauriNativeBridge) {
      nativeBridge.attachEventEmitter(client.events as DefaultEventsApi);
    }
    return client;
  }

  static createClientWithBridge(bridge: NativeBridge): FlareImClient {
    return FlareCoreSdk.createClient(bridge);
  }
}
