// TODO: C FFI bindings need full redesign for new event-stream API.
//
// New API entry point: flare_im_core_sdk::client::FlareImClient
// New store traits: MessageStore, ConversationStore, SyncCursorStore
// New event system: SdkEvent via EventStream (broadcast)
//
// See flare_im_core_sdk::prelude for the complete public API.
