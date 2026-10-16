import Foundation

/// GENERATED. Do not edit by hand.
public final class DefaultFlareImClient: FlareImClientProtocol {
    public let connection: any ConnectionApiProtocol
    public let conversations: any ConversationsApiProtocol
    public let messageBuilder: any MessageBuilderApiProtocol
    public let messages: any MessagesApiProtocol
    public let sync: any SyncApiProtocol
    public let user: any UserApiProtocol
    public let presence: any PresenceApiProtocol
    public let media: any MediaApiProtocol
    public let capabilities: any CapabilitiesApiProtocol
    public let views: any ViewsApiProtocol
    public let events: any EventsApiProtocol
    public let diagnostics: any DiagnosticsApiProtocol
    private let bridge: any NativeBridgeProtocol

    public init(bridge: any NativeBridgeProtocol) {
        self.bridge = bridge
        self.connection = DefaultConnectionApi(bridge: bridge)
        self.conversations = DefaultConversationsApi(bridge: bridge)
        self.messageBuilder = DefaultMessageBuilderApi(bridge: bridge)
        self.messages = DefaultMessagesApi(bridge: bridge)
        self.sync = DefaultSyncApi(bridge: bridge)
        self.user = DefaultUserApi(bridge: bridge)
        self.presence = DefaultPresenceApi(bridge: bridge)
        self.media = DefaultMediaApi(bridge: bridge)
        self.capabilities = DefaultCapabilitiesApi(bridge: bridge)
        self.views = DefaultViewsApi(bridge: bridge)
        self.events = DefaultEventsApi(bridge: bridge)
        self.diagnostics = DefaultDiagnosticsApi(bridge: bridge)
    }

    public func create(_ request: [String: AnySendable]) async throws -> [String: AnySendable] {
        return try await invokeMap(bridge, descriptor: NativeCallMap.sdkCreate, request: unwrapRequest(AnySendable(request)))
    }
    public func `init`(_ request: [String: AnySendable]) async throws -> Void {
        (events as? DefaultEventsApi)?.emit(LifecycleEvent(name: .initializing, operation: "sdk.init"))
        do {
            try await invokeVoid(bridge, descriptor: NativeCallMap.sdkInit, request: AnySendable(request))
            (events as? DefaultEventsApi)?.emit(LifecycleEvent(name: .initialized, operation: "sdk.init"))
        } catch {
            (events as? DefaultEventsApi)?.emit(LifecycleEvent(
                name: .initFailed,
                operation: "sdk.init",
                error: sdkErrorPayload(from: error, operation: "sdk.init")
            ))
            throw error
        }
    }
    public func uninit() async throws -> Void {
        try await invokeVoid(bridge, descriptor: NativeCallMap.sdkUninit, request: nil)
    }
    public func login(_ request: [String: AnySendable]) async throws -> Void {
        let userId = userIdFromRequest(request)
        do {
            try await invokeVoid(bridge, descriptor: NativeCallMap.sdkLogin, request: AnySendable(request))
            (events as? DefaultEventsApi)?.emit(LifecycleEvent(name: .loginSucceeded, operation: "sdk.login", userId: userId))
        } catch {
            (events as? DefaultEventsApi)?.emit(LifecycleEvent(
                name: .loginFailed,
                operation: "sdk.login",
                userId: userId,
                error: sdkErrorPayload(from: error, operation: "sdk.login")
            ))
            throw error
        }
    }
    public func prepare(_ request: [String: AnySendable]) async throws -> Void {
        try await invokeVoid(bridge, descriptor: NativeCallMap.sdkPrepare, request: AnySendable(request))
    }
    public func connect(_ request: [String: AnySendable]) async throws -> Void {
        try await invokeVoid(bridge, descriptor: NativeCallMap.sdkConnect, request: AnySendable(request))
    }
    public func updateAccessToken(_ request: [String: AnySendable]) async throws -> Void {
        try await invokeVoid(bridge, descriptor: NativeCallMap.sdkUpdateAccessToken, request: AnySendable(request))
    }
    public func setHeartbeatAppState(_ request: SetHeartbeatAppStateRequest) async throws -> Void {
        try await invokeVoid(bridge, descriptor: NativeCallMap.sdkSetHeartbeatAppState, request: AnySendable(setHeartbeatAppStateRequestToMap(request)))
    }
    public func setHeartbeatNatTimeout(_ request: SetHeartbeatNatTimeoutRequest) async throws -> Void {
        try await invokeVoid(bridge, descriptor: NativeCallMap.sdkSetHeartbeatNatTimeout, request: AnySendable(setHeartbeatNatTimeoutRequestToMap(request)))
    }
    public func heartbeatEffectiveInterval() async throws -> HeartbeatEffectiveIntervalResponse {
        let raw = try await invokeMap(bridge, descriptor: NativeCallMap.sdkHeartbeatEffectiveInterval, request: nil)
        return try heartbeatEffectiveIntervalResponseFromJson(raw)
    }
    public func updatePushToken(_ request: PushTokenInput) async throws -> PushLifecycleStatus {
        let raw = try await invokeMap(bridge, descriptor: NativeCallMap.sdkUpdatePushToken, request: ["provider": wrapSendable(request.provider), "platform": wrapSendable(request.platform), "token": wrapSendable(request.token)])
        return try JSONDecoder().decode(PushLifecycleStatus.self, from: JSONSerialization.data(withJSONObject: plainMap(raw)))
    }

    public func clearPushToken(_ request: ClearPushTokenRequest) async throws -> PushLifecycleStatus {
        let raw = try await invokeMap(bridge, descriptor: NativeCallMap.sdkClearPushToken, request: ["provider": wrapSendable(request.provider)])
        return try JSONDecoder().decode(PushLifecycleStatus.self, from: JSONSerialization.data(withJSONObject: plainMap(raw)))
    }

    public func setPushPermission(_ request: SetPushPermissionRequest) async throws -> PushLifecycleStatus {
        let raw = try await invokeMap(bridge, descriptor: NativeCallMap.sdkSetPushPermission, request: ["allowed": wrapSendable(request.allowed)])
        return try JSONDecoder().decode(PushLifecycleStatus.self, from: JSONSerialization.data(withJSONObject: plainMap(raw)))
    }

    public func reconcilePush() async throws -> PushLifecycleStatus {
        let raw = try await invokeMap(bridge, descriptor: NativeCallMap.sdkReconcilePush, request: nil)
        return try JSONDecoder().decode(PushLifecycleStatus.self, from: JSONSerialization.data(withJSONObject: plainMap(raw)))
    }

    public func pushStatus() async throws -> PushLifecycleStatus {
        let raw = try await invokeMap(bridge, descriptor: NativeCallMap.sdkPushStatus, request: nil)
        return try JSONDecoder().decode(PushLifecycleStatus.self, from: JSONSerialization.data(withJSONObject: plainMap(raw)))
    }

    public func logout() async throws -> Void {
        try await invokeVoid(bridge, descriptor: NativeCallMap.sdkLogout, request: nil)
        (events as? DefaultEventsApi)?.emit(LifecycleEvent(name: .loggedOut, operation: "sdk.logout"))
    }
    public func dispose() async throws -> Void {
        try await events.unsubscribeAll()
        try await invokeVoid(bridge, descriptor: NativeCallMap.sdkDispose, request: nil)
        (events as? DefaultEventsApi)?.emit(LifecycleEvent(name: .disposed, operation: "sdk.dispose"))
    }
    public func hardReset() async throws -> Void {
        try await invokeVoid(bridge, descriptor: NativeCallMap.sdkHardReset, request: nil)
    }
    public func currentUserId() async throws -> [String: AnySendable] {
        return try await invokeMap(bridge, descriptor: NativeCallMap.sdkCurrentUserId, request: nil)
    }
    public func isConnected() async throws -> Bool {
        return try await invokeBool(bridge, descriptor: NativeCallMap.sdkIsConnected, request: nil)
    }
    public func sessionActive() async throws -> Bool {
        return try await invokeBool(bridge, descriptor: NativeCallMap.sdkSessionActive, request: nil)
    }
}
