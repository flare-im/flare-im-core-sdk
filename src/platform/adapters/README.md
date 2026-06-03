# Platform Adapters

This directory isolates host-platform differences from IM domain/application logic.

`AdapterPlatform` is the shared host family used by all adapter profiles:

- `web`
- `react_native`
- `uni_app`
- `android`
- `ios`
- `flutter`
- `harmony`
- `native`

Media and storage are not separate platform taxonomies. They are separate
capability profiles for the same host family.

## Layout

- `platform.rs`: shared adapter platform family and provisioning mode.
- `media/profile.rs`: media source support and whether upload/cache is built in or host injected.
- `media/upload_only.rs`: helper service for hosts that only provide upload.
- `media/native_file_service.rs`: native file-path upload/cache/download service.
- `media/web_stub_service.rs`: wasm stub that forces Web hosts to inject a media adapter.
- `storage/profile.rs`: preferred storage backend and provisioning mode.
- `storage/store_factory.rs`: built-in store opening from runtime config.

Core SDK business code should depend on ports under `platform/ports`, not on
these concrete adapters directly.
