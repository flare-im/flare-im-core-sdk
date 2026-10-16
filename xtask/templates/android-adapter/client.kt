package com.flare.im.adapter

/** GENERATED. Do not edit by hand. */

import com.flare.im.adapter.codec.*
import com.flare.im.adapter.module.*
import com.flare.im.api.FlareImClient
import com.flare.im.api.connection.ConnectionApi
import com.flare.im.api.conversations.ConversationsApi
import com.flare.im.api.messagebuilder.MessageBuilderApi
import com.flare.im.api.messages.MessagesApi
import com.flare.im.api.sync.SyncApi
import com.flare.im.api.user.UserApi
import com.flare.im.api.presence.PresenceApi
import com.flare.im.api.media.MediaApi
import com.flare.im.api.capabilities.CapabilitiesApi
import com.flare.im.api.views.ViewsApi
import com.flare.im.api.events.EventsApi
import com.flare.im.api.diagnostics.DiagnosticsApi
import com.flare.im.contract.NativeBridge
import com.flare.im.contract.NativeCallMap
import com.flare.im.model.catalog.*
import com.flare.im.model.command.*
import com.flare.im.model.command.message.*
import com.flare.im.model.command.message.build.*
import com.flare.im.model.common.enums.*
import com.flare.im.model.common.error.*
import com.flare.im.model.content.*
import com.flare.im.model.entity.*
import com.flare.im.model.event.*
import com.flare.im.model.event.capability.*
import com.flare.im.model.event.connection.*
import com.flare.im.model.event.conversation.*
import com.flare.im.model.event.lifecycle.*
import com.flare.im.model.event.message.*
import com.flare.im.model.event.presence.*
import com.flare.im.model.event.progress.*
import com.flare.im.model.event.sync.*
import com.flare.im.model.media.*
import com.flare.im.model.query.*
import com.flare.im.model.response.*

class DefaultFlareImClient(
    private val bridge: NativeBridge,
) : FlareImClient {

    override val connection: ConnectionApi = DefaultConnectionApi(bridge)
    override val conversations: ConversationsApi = DefaultConversationsApi(bridge)
    override val messageBuilder: MessageBuilderApi = DefaultMessageBuilderApi(bridge)
    override val messages: MessagesApi = DefaultMessagesApi(bridge)
    override val sync: SyncApi = DefaultSyncApi(bridge)
    override val user: UserApi = DefaultUserApi(bridge)
    override val presence: PresenceApi = DefaultPresenceApi(bridge)
    override val media: MediaApi = DefaultMediaApi(bridge)
    override val capabilities: CapabilitiesApi = DefaultCapabilitiesApi(bridge)
    override val views: ViewsApi = DefaultViewsApi(bridge)
    override val events: EventsApi = DefaultEventsApi(bridge)
    override val diagnostics: DiagnosticsApi = DefaultDiagnosticsApi(bridge)

    override suspend fun create(request: Map<String, Any?>): Map<String, Any?> {
        return invokeMap(bridge, NativeCallMap.SDK_CREATE, request)
    }
    override suspend fun init(request: Map<String, Any?>): Unit {
        (events as DefaultEventsApi).emitLifecycleEvent(
            LifecycleEventName.INITIALIZING,
            "sdk.init",
        )
        try {
            invokeUnit(bridge, NativeCallMap.SDK_INIT, request)
            (events as DefaultEventsApi).emitLifecycleEvent(
                LifecycleEventName.INITIALIZED,
                "sdk.init",
            )
        } catch (error: Throwable) {
            (events as DefaultEventsApi).emitLifecycleEvent(
                LifecycleEventName.INIT_FAILED,
                "sdk.init",
                error = sdkErrorPayloadFromThrowable(error, "sdk.init"),
            )
            throw error
        }
    }
    override suspend fun uninit(): Unit {
        invokeUnit(bridge, NativeCallMap.SDK_UNINIT)
    }
    override suspend fun login(request: Map<String, Any?>): Unit {
        val userId = userIdFromRequest(request)
        try {
            invokeUnit(bridge, NativeCallMap.SDK_LOGIN, request)
            (events as DefaultEventsApi).emitLifecycleEvent(
                LifecycleEventName.LOGIN_SUCCEEDED,
                "sdk.login",
                userId = userId,
            )
        } catch (error: Throwable) {
            (events as DefaultEventsApi).emitLifecycleEvent(
                LifecycleEventName.LOGIN_FAILED,
                "sdk.login",
                userId = userId,
                error = sdkErrorPayloadFromThrowable(error, "sdk.login"),
            )
            throw error
        }
    }
    override suspend fun prepare(request: Map<String, Any?>): Unit {
        invokeUnit(bridge, NativeCallMap.SDK_PREPARE, request)
    }
    override suspend fun connect(request: Map<String, Any?>): Unit {
        invokeUnit(bridge, NativeCallMap.SDK_CONNECT, request)
    }
    override suspend fun updateAccessToken(request: Map<String, Any?>): Unit {
        invokeUnit(bridge, NativeCallMap.SDK_UPDATE_ACCESS_TOKEN, request)
    }
    override suspend fun setHeartbeatAppState(request: SetHeartbeatAppStateRequest): Unit {
        invokeUnit(bridge, NativeCallMap.SDK_SET_HEARTBEAT_APP_STATE, setHeartbeatAppStateRequestToMap(request))
    }
    override suspend fun setHeartbeatNatTimeout(request: SetHeartbeatNatTimeoutRequest): Unit {
        invokeUnit(bridge, NativeCallMap.SDK_SET_HEARTBEAT_NAT_TIMEOUT, setHeartbeatNatTimeoutRequestToMap(request))
    }
    override suspend fun heartbeatEffectiveInterval(): HeartbeatEffectiveIntervalResponse {
        return heartbeatEffectiveIntervalResponseFromJson(invokeMap(bridge, NativeCallMap.SDK_HEARTBEAT_EFFECTIVE_INTERVAL))
    }
    override suspend fun updatePushToken(request: PushTokenInput): PushLifecycleStatus {
        val raw = invokeMap(bridge, NativeCallMap.SDK_UPDATE_PUSH_TOKEN, mapOf("provider" to request.provider, "platform" to request.platform, "token" to request.token))
        return PushLifecycleStatus(configuredProviders = intValue(raw["configuredProviders"]), pendingRevocations = intValue(raw["pendingRevocations"]), registeredProviders = intValue(raw["registeredProviders"]), state = raw["state"] as String, lastError = raw["lastError"] as? String)
    }

    override suspend fun clearPushToken(request: ClearPushTokenRequest): PushLifecycleStatus {
        val raw = invokeMap(bridge, NativeCallMap.SDK_CLEAR_PUSH_TOKEN, mapOf("provider" to request.provider))
        return PushLifecycleStatus(configuredProviders = intValue(raw["configuredProviders"]), pendingRevocations = intValue(raw["pendingRevocations"]), registeredProviders = intValue(raw["registeredProviders"]), state = raw["state"] as String, lastError = raw["lastError"] as? String)
    }

    override suspend fun setPushPermission(request: SetPushPermissionRequest): PushLifecycleStatus {
        val raw = invokeMap(bridge, NativeCallMap.SDK_SET_PUSH_PERMISSION, mapOf("allowed" to request.allowed))
        return PushLifecycleStatus(configuredProviders = intValue(raw["configuredProviders"]), pendingRevocations = intValue(raw["pendingRevocations"]), registeredProviders = intValue(raw["registeredProviders"]), state = raw["state"] as String, lastError = raw["lastError"] as? String)
    }

    override suspend fun reconcilePush(): PushLifecycleStatus {
        val raw = invokeMap(bridge, NativeCallMap.SDK_RECONCILE_PUSH)
        return PushLifecycleStatus(configuredProviders = intValue(raw["configuredProviders"]), pendingRevocations = intValue(raw["pendingRevocations"]), registeredProviders = intValue(raw["registeredProviders"]), state = raw["state"] as String, lastError = raw["lastError"] as? String)
    }

    override suspend fun pushStatus(): PushLifecycleStatus {
        val raw = invokeMap(bridge, NativeCallMap.SDK_PUSH_STATUS)
        return PushLifecycleStatus(configuredProviders = intValue(raw["configuredProviders"]), pendingRevocations = intValue(raw["pendingRevocations"]), registeredProviders = intValue(raw["registeredProviders"]), state = raw["state"] as String, lastError = raw["lastError"] as? String)
    }

    override suspend fun logout(): Unit {
        invokeUnit(bridge, NativeCallMap.SDK_LOGOUT)
        (events as DefaultEventsApi).emitLifecycleEvent(
            LifecycleEventName.LOGGED_OUT,
            "sdk.logout",
        )
    }
    override suspend fun dispose(): Unit {
        events.unsubscribeAll()
        invokeUnit(bridge, NativeCallMap.SDK_DISPOSE)
        (events as DefaultEventsApi).emitLifecycleEvent(
            LifecycleEventName.DISPOSED,
            "sdk.dispose",
        )
    }
    override suspend fun hardReset(): Unit {
        invokeUnit(bridge, NativeCallMap.SDK_HARD_RESET)
    }
    override suspend fun currentUserId(): Map<String, Any?> {
        return invokeMap(bridge, NativeCallMap.SDK_CURRENT_USER_ID)
    }
    override suspend fun isConnected(): Boolean {
        return invokeBool(bridge, NativeCallMap.SDK_IS_CONNECTED)
    }
    override suspend fun sessionActive(): Boolean {
        return invokeBool(bridge, NativeCallMap.SDK_SESSION_ACTIVE)
    }
}
