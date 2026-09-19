# Message search contract

All three binding operations (`message.search`, `message.search_in_conversation`,
`message.search_by_query`) consume the complete `MessageSearchQuery` and dispatch
to `MessageApi::search_by_query`. Their existing names remain supported. No
binding may reduce this object to only conversation ID, keyword and limit.
This applies to the shared dispatcher used by WASM, C FFI/JNI and native clients.

Query constraints combine with AND: conversation, sender, time range (inclusive),
keyword, recalled visibility, and type selection. Multiple kinds combine with OR.
An empty kinds array or `message` means no type restriction. `text` includes Text,
RichText and Quote; `image` includes Image and ImageGroup; `media` includes Image,
ImageGroup, Video, Audio and File. A JPG sent as a File remains in the File category;
classification uses the canonical message type, not the filename extension.

Filtering runs before sorting and limit (clamped to 1–200). Recalled messages are
excluded by default. Empty keywords permit a type-only query. Memory search uses
case-insensitive substring matching over typed searchable content and contentText;
SQLite retains its existing FTS/short-keyword plan. Both reuse the same type mapping
and typed content extraction. Search time is createdAt, falling back to
clientCreatedAt, then local sortTs. Results order by that time descending and then
conversationSeq descending.

The Web IndexedDB host wrapper forwards the full query to its in-memory view.
Memory global search also applies the keyword. Storage implementations without
full-query support return OperationNotSupported instead of silently dropping
constraints. The empty fixture store explicitly returns an empty result.

Regression commands:

```sh
cargo test -p flare-im-core-sdk --lib
cargo test -p flare-im-core-sdk --features storage-sqlite --lib search
cargo test -p flare-im-core-sdk-bindings-runtime --lib
cargo xtask core-codegen-check
```

In the client SDK checkout, after building WASM:

```sh
node scripts/check-wasm-search-filters.mjs
node scripts/check-wasm-search-filters.mjs https://118-107-9-221.sslip.io
```

The browser matrix loads isolated fixture messages through the storage host,
exercises all three search operations and all seven categories plus compound
conditions, and never sends fixture messages to the production server.
