# Session and invocation recovery

The Rust IMClient owns local session identity independently of transport state.
After `prepare`, local message queries are available while disconnected or
reconnecting. This does not authorize network requests or imply the transport is
ready. Logout, account replacement and authentication termination invalidate old
facades. Each facade snapshot carries the generation at which it was captured;
bindings must never label an old snapshot with the current generation.

## Web/WASM

The TypeScript adapter serializes invocations and reports the existing operation
deadline to the caller. It retains the queue slot until the underlying invocation
settles. After a five-second grace period it requests cooperative cancellation
through `cancelPendingInvocations`, then waits another five seconds. If the
invocation still has not settled, subsequent operations fail with
`wasm.recovery_pending` until it settles. No timeout silently replaces a runtime,
replays login, or invents a connected state. Session predicates return the core's
boolean without overriding it using cached connection events.

Cancellation is scoped to the runtime. `sdk.*` and `connection.*` operations are
excluded because their futures can temporarily own an engine during lifecycle
transitions. Cancelling other invocations releases their local waiter; remote
writes may already have committed and must not be replayed automatically.

Event callbacks are registered per runtime. Explicit disposal removes that
runtime's callback and event bridge, cancels ordinary pending invocations, and
awaits logout. Logout alone retains the event callback for subsequent login.

The WASM executor remains cooperative and shares a module-level driver. A blocked
JavaScript thread or non-yielding WASM code cannot be recovered by a JavaScript
timer. Worker termination/isolation is outside this implementation. Disposing a
runtime is not a promise of recovery from a permanently stuck lifecycle future.

## Native C FFI

Ordinary invocations have a 120-second local deadline and complete once with
`OperationTimeout` when exceeded. Lifecycle operations are excluded. Releasing a
handle cancels ordinary pending invocations and schedules logout. Results from
an obsolete session generation are rejected. The instance is never silently
replaced, and writes are never automatically replayed on timeout.

Global hard reset disables outstanding result callbacks before releasing
instances, preventing later completions from targeting invalid host callback
addresses. Hosts must coordinate reset with callback execution; this atomic gate
does not revoke a callback already executing on another thread.

Android JNI, Apple and Flutter inherit shared Rust session/generation behavior.
The additional deadline and callback gate described here apply to C FFI consumers;
they do not imply a new timeout policy for every other native binding.

## Regression coverage

- Prepared/disconnected sessions can search local messages.
- Preparing another account advances generation and invalidates old APIs.
- Logout invalidates current APIs.
- Runtime cancellation does not affect another runtime or future invocations.
- Lifecycle futures are not cancelled while owning the engine.
- Web timeout retains the authenticated instance and permits queries after cancellation.
- Unacknowledged cancellation blocks new calls until the original invocation settles.
- A cached Ready event never changes a core false session predicate to true.
- Native timeout and release complete waiting queries once; hard reset suppresses callbacks.

## Validation (2026-09-09)

- Core SDK library: 465 tests passed.
- Shared bindings runtime: 48 tests passed, including delayed old-account cache installation.
- C FFI library: 11 tests passed.
- TypeScript Web bridge/client/priority: 13 tests passed; TypeScript typecheck passed.
- `cargo xtask bridge-check` passed.
- `cargo xtask build wasm` and Web example `npm run build:web` passed.
- Client SDK `node scripts/check-wasm-session-recovery.mjs` passed in Chromium
  against the compiled WASM: offline search, cancellation and subsequent search,
  independent instances, disposal, and logout invalidation.
- Web dist JS/WASM bytes match the generated WASM package.

The browser regression requires a browser environment. A preliminary bare Node
probe stalled during prepare and was stopped; Node is not substituted for the
browser's window/timer runtime in this validation.

No production deployment or Android/iOS/Flutter device validation was performed.
Native consumers must rebuild and distribute their corresponding native artifacts
to receive the shared Rust changes; changing the Web dist cannot update them.
