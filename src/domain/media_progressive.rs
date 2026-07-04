use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaProgressiveStage {
    Placeholder,
    Blurhash,
    Thumbnail,
    LowResolution,
    FullResolution,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaProgressiveEvent {
    pub file_id: String,
    pub stage: MediaProgressiveStage,
    pub blurhash: Option<String>,
    pub cache_key: Option<String>,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaProgressiveState {
    pub file_id: String,
    pub stage: MediaProgressiveStage,
    pub blurhash: Option<String>,
    pub thumbnail_cache_key: Option<String>,
    pub low_resolution_cache_key: Option<String>,
    pub full_resolution_cache_key: Option<String>,
    pub updated_at_ms: i64,
}

impl MediaProgressiveState {
    pub fn placeholder(file_id: impl Into<String>, updated_at_ms: i64) -> Self {
        Self {
            file_id: file_id.into(),
            stage: MediaProgressiveStage::Placeholder,
            blurhash: None,
            thumbnail_cache_key: None,
            low_resolution_cache_key: None,
            full_resolution_cache_key: None,
            updated_at_ms,
        }
    }

    pub fn apply_blurhash(
        &mut self,
        blurhash: impl Into<String>,
        updated_at_ms: i64,
    ) -> Option<MediaProgressiveEvent> {
        let blurhash = blurhash.into();
        self.advance(
            MediaProgressiveStage::Blurhash,
            Some(blurhash.clone()),
            None,
            updated_at_ms,
            |state| {
                state.blurhash = Some(blurhash);
            },
        )
    }

    pub fn apply_thumbnail(
        &mut self,
        cache_key: impl Into<String>,
        updated_at_ms: i64,
    ) -> Option<MediaProgressiveEvent> {
        let cache_key = cache_key.into();
        self.advance(
            MediaProgressiveStage::Thumbnail,
            None,
            Some(cache_key.clone()),
            updated_at_ms,
            |state| {
                state.thumbnail_cache_key = Some(cache_key);
            },
        )
    }

    pub fn apply_low_resolution(
        &mut self,
        cache_key: impl Into<String>,
        updated_at_ms: i64,
    ) -> Option<MediaProgressiveEvent> {
        let cache_key = cache_key.into();
        self.advance(
            MediaProgressiveStage::LowResolution,
            None,
            Some(cache_key.clone()),
            updated_at_ms,
            |state| {
                state.low_resolution_cache_key = Some(cache_key);
            },
        )
    }

    pub fn apply_full_resolution(
        &mut self,
        cache_key: impl Into<String>,
        updated_at_ms: i64,
    ) -> Option<MediaProgressiveEvent> {
        let cache_key = cache_key.into();
        self.advance(
            MediaProgressiveStage::FullResolution,
            None,
            Some(cache_key.clone()),
            updated_at_ms,
            |state| {
                state.full_resolution_cache_key = Some(cache_key);
            },
        )
    }

    fn advance(
        &mut self,
        next_stage: MediaProgressiveStage,
        blurhash: Option<String>,
        cache_key: Option<String>,
        updated_at_ms: i64,
        apply: impl FnOnce(&mut Self),
    ) -> Option<MediaProgressiveEvent> {
        if next_stage < self.stage {
            return None;
        }

        apply(self);
        self.stage = next_stage;
        self.updated_at_ms = self.updated_at_ms.max(updated_at_ms);

        Some(MediaProgressiveEvent {
            file_id: self.file_id.clone(),
            stage: self.stage,
            blurhash,
            cache_key,
            updated_at_ms: self.updated_at_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{MediaProgressiveStage, MediaProgressiveState};

    #[test]
    fn progressive_state_advances_in_display_order() {
        let mut state = MediaProgressiveState::placeholder("file-1", 10);

        let blurhash = state
            .apply_blurhash("LKO2?U%2Tw=w]~RBVZRi};RPxuwH", 11)
            .expect("blurhash event");
        assert_eq!(blurhash.stage, MediaProgressiveStage::Blurhash);
        assert_eq!(state.stage, MediaProgressiveStage::Blurhash);

        let thumbnail = state
            .apply_thumbnail("cache://thumb/file-1", 12)
            .expect("thumbnail event");
        assert_eq!(thumbnail.stage, MediaProgressiveStage::Thumbnail);

        let low = state
            .apply_low_resolution("cache://low/file-1", 13)
            .expect("low resolution event");
        assert_eq!(low.stage, MediaProgressiveStage::LowResolution);

        let full = state
            .apply_full_resolution("cache://full/file-1", 14)
            .expect("full resolution event");
        assert_eq!(full.stage, MediaProgressiveStage::FullResolution);
        assert_eq!(state.stage, MediaProgressiveStage::FullResolution);
    }

    #[test]
    fn progressive_state_never_downgrades_display_stage() {
        let mut state = MediaProgressiveState::placeholder("file-1", 10);

        assert!(
            state
                .apply_low_resolution("cache://low/file-1", 11)
                .is_some()
        );
        assert!(state.apply_thumbnail("cache://thumb/file-1", 12).is_none());

        assert_eq!(state.stage, MediaProgressiveStage::LowResolution);
        assert_eq!(
            state.low_resolution_cache_key.as_deref(),
            Some("cache://low/file-1")
        );
        assert!(state.thumbnail_cache_key.is_none());
    }
}
