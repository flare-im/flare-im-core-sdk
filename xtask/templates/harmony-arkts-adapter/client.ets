// GENERATED. Do not edit by hand.
import { NativeBridge, NativeCallMap } from '../contract/BridgeContract';
import type { FlareImClient } from '../api/client';
import type { ConnectionApi } from '../api/modules/connection';
import type { ConversationsApi } from '../api/modules/conversations';
import type { MessageBuilderApi } from '../api/modules/message_builder';
import type { MessagesApi } from '../api/modules/messages';
import type { SyncApi } from '../api/modules/sync';
import type { PresenceApi } from '../api/modules/presence';
import type { MediaApi } from '../api/modules/media';
import type { CapabilitiesApi } from '../api/modules/capabilities';
import type { EventsApi } from '../api/modules/events';
import type { DiagnosticsApi } from '../api/modules/diagnostics';
import { LifecycleEventName } from '../model';
import { emitLifecycleEvent, invokeBool, invokeMap, invokeVoid, sdkErrorPayloadFromError, userIdFromRequest } from './NativeInvoke';
import { DefaultEventsApi } from './DefaultEventsApi';
import { DefaultConnectionApi } from './DefaultConnectionApi';
import { DefaultConversationsApi } from './DefaultConversationsApi';
import { DefaultMessagesApi } from './DefaultMessagesApi';
import { DefaultSyncApi } from './DefaultSyncApi';
import { DefaultPresenceApi } from './DefaultPresenceApi';
import { DefaultMediaApi } from './DefaultMediaApi';
import { DefaultCapabilitiesApi } from './DefaultCapabilitiesApi';
import { DefaultDiagnosticsApi } from './DefaultDiagnosticsApi';
import { DefaultMessageBuilderApi } from './DefaultMessageBuilderApi';

import { PushLifecycleStatus, PushTokenInput, ClearPushTokenRequest, SetPushPermissionRequest } from '../model';

export class DefaultFlareImClient implements FlareImClient {
  readonly connection: ConnectionApi;
  readonly conversations: ConversationsApi;
  readonly messageBuilder: MessageBuilderApi;
  readonly messages: MessagesApi;
  readonly sync: SyncApi;
  readonly presence: PresenceApi;
  readonly media: MediaApi;
  readonly capabilities: CapabilitiesApi;
  readonly events: EventsApi;
  readonly diagnostics: DiagnosticsApi;
  private readonly bridge: NativeBridge;

  constructor(bridge: NativeBridge) {
    this.bridge = bridge;
    this.connection = new DefaultConnectionApi(bridge);
    this.conversations = new DefaultConversationsApi(bridge);
    this.messageBuilder = new DefaultMessageBuilderApi(bridge);
    this.messages = new DefaultMessagesApi(bridge);
    this.sync = new DefaultSyncApi(bridge);
    this.presence = new DefaultPresenceApi(bridge);
    this.media = new DefaultMediaApi(bridge);
    this.capabilities = new DefaultCapabilitiesApi(bridge);
    this.events = new DefaultEventsApi(bridge);
    this.diagnostics = new DefaultDiagnosticsApi(bridge);
  }

  async create(request: Record<string, Object>): Promise<Record<string, Object>> {
    return await invokeMap(this.bridge, NativeCallMap.sdkCreate, request);
  }
  async init(request: Record<string, Object>): Promise<void> {
    emitLifecycleEvent(this.events as DefaultEventsApi, LifecycleEventName.Initializing, "sdk.init");
    try {
      await invokeVoid(this.bridge, NativeCallMap.sdkInit, request);
      emitLifecycleEvent(this.events as DefaultEventsApi, LifecycleEventName.Initialized, "sdk.init");
    } catch (error) {
      emitLifecycleEvent(
        this.events as DefaultEventsApi,
        LifecycleEventName.InitFailed,
        "sdk.init",
        undefined,
        sdkErrorPayloadFromError(error as Object, "sdk.init"),
      );
      throw error;
    }
  }
  async uninit(): Promise<void> {
    await invokeVoid(this.bridge, NativeCallMap.sdkUninit);
  }
  async login(request: Record<string, Object>): Promise<void> {
    const userId = userIdFromRequest(request);
    try {
      await invokeVoid(this.bridge, NativeCallMap.sdkLogin, request);
      emitLifecycleEvent(this.events as DefaultEventsApi, LifecycleEventName.LoginSucceeded, "sdk.login", userId);
    } catch (error) {
      emitLifecycleEvent(
        this.events as DefaultEventsApi,
        LifecycleEventName.LoginFailed,
        "sdk.login",
        userId,
        sdkErrorPayloadFromError(error as Object, "sdk.login"),
      );
      throw error;
    }
  }
  async prepare(request: Record<string, Object>): Promise<void> {
    await invokeVoid(this.bridge, NativeCallMap.sdkPrepare, request);
  }
  async connect(request: Record<string, Object>): Promise<void> {
    await invokeVoid(this.bridge, NativeCallMap.sdkConnect, request);
  }
  async updateAccessToken(request: Record<string, Object>): Promise<void> {
    await invokeVoid(this.bridge, NativeCallMap.sdkUpdateAccessToken, request);
  }
  async setHeartbeatAppState(request: SetHeartbeatAppStateRequest): Promise<void> {
    await invokeVoid(this.bridge, NativeCallMap.sdkSetHeartbeatAppState, setHeartbeatAppStateRequestToMap(request));
  }
  async setHeartbeatNatTimeout(request: SetHeartbeatNatTimeoutRequest): Promise<void> {
    await invokeVoid(this.bridge, NativeCallMap.sdkSetHeartbeatNatTimeout, setHeartbeatNatTimeoutRequestToMap(request));
  }
  async heartbeatEffectiveInterval(): Promise<HeartbeatEffectiveIntervalResponse> {
    return await this.bridge.invoke<HeartbeatEffectiveIntervalResponse>(NativeCallMap.sdkHeartbeatEffectiveInterval);
  }
  async updatePushToken(request: PushTokenInput): Promise<PushLifecycleStatus> {
    const raw = await invokeMap(this.bridge, NativeCallMap.sdkUpdatePushToken, { provider: request.provider, platform: request.platform, token: request.token });
    return { configuredProviders: raw['configuredProviders'] as number, pendingRevocations: raw['pendingRevocations'] as number, registeredProviders: raw['registeredProviders'] as number, state: raw['state'] as string, lastError: raw['lastError'] as string | undefined };
  }

  async clearPushToken(request: ClearPushTokenRequest): Promise<PushLifecycleStatus> {
    const raw = await invokeMap(this.bridge, NativeCallMap.sdkClearPushToken, { provider: request.provider });
    return { configuredProviders: raw['configuredProviders'] as number, pendingRevocations: raw['pendingRevocations'] as number, registeredProviders: raw['registeredProviders'] as number, state: raw['state'] as string, lastError: raw['lastError'] as string | undefined };
  }

  async setPushPermission(request: SetPushPermissionRequest): Promise<PushLifecycleStatus> {
    const raw = await invokeMap(this.bridge, NativeCallMap.sdkSetPushPermission, { allowed: request.allowed });
    return { configuredProviders: raw['configuredProviders'] as number, pendingRevocations: raw['pendingRevocations'] as number, registeredProviders: raw['registeredProviders'] as number, state: raw['state'] as string, lastError: raw['lastError'] as string | undefined };
  }

  async reconcilePush(): Promise<PushLifecycleStatus> {
    const raw = await invokeMap(this.bridge, NativeCallMap.sdkReconcilePush);
    return { configuredProviders: raw['configuredProviders'] as number, pendingRevocations: raw['pendingRevocations'] as number, registeredProviders: raw['registeredProviders'] as number, state: raw['state'] as string, lastError: raw['lastError'] as string | undefined };
  }

  async pushStatus(): Promise<PushLifecycleStatus> {
    const raw = await invokeMap(this.bridge, NativeCallMap.sdkPushStatus);
    return { configuredProviders: raw['configuredProviders'] as number, pendingRevocations: raw['pendingRevocations'] as number, registeredProviders: raw['registeredProviders'] as number, state: raw['state'] as string, lastError: raw['lastError'] as string | undefined };
  }

  async logout(): Promise<void> {
    await invokeVoid(this.bridge, NativeCallMap.sdkLogout);
    emitLifecycleEvent(this.events as DefaultEventsApi, LifecycleEventName.LoggedOut, "sdk.logout");
  }
  async dispose(): Promise<void> {
    await this.events.unsubscribeAll();
    await invokeVoid(this.bridge, NativeCallMap.sdkDispose);
    emitLifecycleEvent(this.events as DefaultEventsApi, LifecycleEventName.Disposed, "sdk.dispose");
  }
  async hardReset(): Promise<void> {
    await invokeVoid(this.bridge, NativeCallMap.sdkHardReset);
  }
  async currentUserId(): Promise<Record<string, Object>> {
    return await invokeMap(this.bridge, NativeCallMap.sdkCurrentUserId);
  }
  async isConnected(): Promise<boolean> {
    return await invokeBool(this.bridge, NativeCallMap.sdkIsConnected);
  }
  async sessionActive(): Promise<boolean> {
    return await invokeBool(this.bridge, NativeCallMap.sdkSessionActive);
  }
}
