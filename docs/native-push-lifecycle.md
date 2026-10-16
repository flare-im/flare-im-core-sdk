# Native push lifecycle

Rust owns binding identity, CAS, unknown-result recovery and logout/account-switch safety.
The OS/plugin owns permission, token refresh, channels, background execution and display.
Do not implement another registration/retry state machine in Flutter or an example app.

## Normal SDK contract

Before login, configure a private persistent `dataUrl`, the actual Core `httpUrl`/TLS trust,
and a stable `deviceId`. Use the same device ID to obtain the device-bound IM connect token.
Do not rely on the fallback ephemeral device. HTTPS is required except for local development
loopback/localhost. Core HTTP does not follow redirects; credentials/write bodies stay at the
configured destination. Keep the deployment URL/data root stable while cleanup is pending.

Generated APIs are `updatePushToken(PushTokenInput)`, `clearPushToken({provider})`,
`setPushPermission({allowed})`, `reconcilePush()` and `pushStatus()` (Rust names snake_case).
Providers are `fcm`, `apns`, `getui`; platforms `android`, `ios`, `harmony`. APNs tokens are hex.
Permission starts false on every process start. A pre-login OS token remains memory-only and
does not register until permission and a matching authenticated app/user/device session exist.

The normal `login/connect`, access-token update and reconnect paths pin immutable auth for
the lifecycle. Normal `logout`, account switch, disconnect and terminal session retirement
detach it immediately and schedule revoke-only cleanup. Desired OS tokens may be reused for
the next account, but the old binding is revoked before a new conditional binding is sent.
Permission denial/token invalidation retires the relevant binding too. Do not await a token
callback before forwarding permission denial: core deliberately permits this interleaving.

The status reports `disabled`, `waitingForSession`, `waitingForToken`, `pending`, or `registered`,
provider counts and a fixed safe error category. `registered` is server binding confirmation,
not vendor acceptance/phone delivery. `pendingRevocations` includes unresolved writes.
Call reconcile on foreground/network recovery and expose pending status instead of claiming
an offline logout already revoked the server. No permanent polling/background OS execution
is promised. A completely terminated/uninstalled application cannot run cleanup.

## Persistence and cancellation

The native write-ahead journal holds only per-attempt revoke capabilities, provider and
destination hash: no OS token, user, bearer or raw URL. It is bounded (64 intents/64 KiB),
exclusive-writer, private 0600 and atomic fsync/replace. Keep its directory under a trusted
private SDK data root; it is not a defense against a hostile process running as the same OS user.
Bad permissions, symlink/alias records, malformed/oversized storage or another writer fail
closed. A different destination/device cannot consume pending old capabilities. Restore the
original configuration and reconcile before changing deployment/device/data root; do not
silently discard pending storage. Browser/WASM OS-token updates explicitly return
OperationNotSupported; this is not a Web Push implementation. Native persistence was verified
on macOS/Unix; Windows private storage/directory fsync is not claimed verified.

Intent is persisted before a possibly committed PUT. After a lost response/cancel, read-back
confirms the exact attempt, or an exact revoke fences it before another CAS. PUT is never
blindly retried. A restarted client treats prior intent as revoke-only and can recover from
the login page without retaining authentication. Requests have a five-second IO budget and
each reconciliation has a 20-second async budget including its serialization wait; blocking
local filesystem calls are small/bounded bytes but not a hard wall-clock guarantee.

The server uses a singleton Redis/Dragonfly authority, 24-hour tombstones and max ten-minute
commands (SDK uses two minutes). Do not reset/restore this authority independently of clients.
Revocation cannot recall a vendor request already sent, or undo a notification already displayed.

## Flutter composition

`NativePushLifecycleBridge` is a plugin-neutral callback adapter, not a Firebase dependency:

```dart
final push = NativePushLifecycleBridge(client);
// Read current OS permission before publishing a token, including explicit denial.
await push.onPermissionChanged(osPermissionGranted);
await push.onTokenRefresh(PushTokenInput(
  provider: 'fcm', platform: 'android', token: osToken,
));
// OS token stream -> onTokenRefresh; token invalidation -> onTokenInvalidated.
// WidgetsBindingObserver resume -> onResume; network recovery -> onNetworkAvailable.
// Use normal client.logout()/login(...) for exit/account switch, no manual DELETE.
```

Host code owns/cancels plugin subscriptions and handles the returned status/errors. It must
not log tokens or substitute the business access token for the connect credential. Explicitly
select the OS provider/project/topic for the current application. Real APNs/FCM credentials,
signed apps/devices, Android notification channels and background/badge behavior are not
provided by these callbacks and still require final integration/device verification.

Local related tests cover real private Redis CAS/revocation, production Gateway HTTP with a
signed controlled tonic peer, actual SDK HTTP/persistence/cancellation boundaries, generated
invoke routes and thin Dart adapters. Controlled peers are not vendor delivery or final R6.
