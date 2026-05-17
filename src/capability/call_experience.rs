//! SDK 侧通话体验描述入口（开源核心 + 商业扩展统一契约）。

#[cfg(feature = "plugin-call")]
pub use flare_sdk_plugin_call::experience::{
    AvExperienceSpec, CallControlSet, CallLayoutMode, ExperienceEdition,
    flare_default_experience_spec, sanitize_experience_spec_for_edition,
};

#[cfg(feature = "plugin-call")]
#[must_use]
pub fn default_call_experience_spec() -> AvExperienceSpec {
    sanitize_experience_spec_for_edition(
        &flare_default_experience_spec(),
        ExperienceEdition::OpenSourceCore,
    )
}
