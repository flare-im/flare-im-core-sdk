//! 时间戳与 `prost_types::Timestamp` 互转工具，统一业务层转换逻辑。

use prost_types::Timestamp;

/// `prost_types::Timestamp` → 毫秒时间戳（u64）。
/// 若为 `None` 或 seconds/nanos 均为 0，返回 0。
#[inline]
pub fn prost_timestamp_to_ms(t: Option<&Timestamp>) -> u64 {
    let Some(x) = t else { return 0 };
    if x.seconds == 0 && x.nanos == 0 {
        return 0;
    }
    if x.seconds < 0 || x.nanos < 0 {
        return 0;
    }
    (x.seconds as u64) * 1000 + (x.nanos as u64) / 1_000_000
}

/// 毫秒时间戳（u64）→ `prost_types::Timestamp`。
/// 若 `ms == 0` 返回 `None`。
#[inline]
pub fn ms_to_prost_timestamp(ms: u64) -> Option<Timestamp> {
    if ms == 0 {
        return None;
    }
    Some(Timestamp {
        seconds: (ms / 1000) as i64,
        nanos: ((ms % 1000) * 1_000_000) as i32,
    })
}

/// `prost_types::Timestamp` → RFC3339 字符串。
/// 若为 `None` 或 seconds/nanos 均为 0，返回空字符串。
#[inline]
pub fn prost_timestamp_to_rfc3339(t: Option<&Timestamp>) -> String {
    let Some(x) = t else { return String::new() };
    prost_timestamp_to_rfc3339_parts(x.seconds, x.nanos)
}

/// (seconds, nanos) → RFC3339 字符串（与 proto Timestamp 字段一致）。
#[inline]
pub fn prost_timestamp_to_rfc3339_parts(seconds: i64, nanos: i32) -> String {
    if seconds == 0 && nanos == 0 {
        return String::new();
    }
    chrono::DateTime::from_timestamp(seconds, nanos as u32)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default()
}

/// RFC3339 字符串 → `prost_types::Timestamp`。
/// 解析失败或空串返回 `None`。
#[inline]
pub fn rfc3339_to_prost_timestamp(s: &str) -> Option<Timestamp> {
    if s.is_empty() {
        return None;
    }
    let dt = chrono::DateTime::parse_from_rfc3339(s).ok()?;
    Some(Timestamp {
        seconds: dt.timestamp(),
        nanos: dt.timestamp_subsec_nanos() as i32,
    })
}

/// 毫秒时间戳（u64）→ RFC3339 字符串。
/// 若 `ms == 0` 返回空字符串。
#[inline]
pub fn ms_to_rfc3339(ms: u64) -> String {
    if ms == 0 {
        return String::new();
    }
    let secs = (ms / 1000) as i64;
    let subsec_nanos = ((ms % 1000) * 1_000_000) as u32;
    chrono::DateTime::from_timestamp(secs, subsec_nanos)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default()
}

/// 当前系统时间 → `prost_types::Timestamp`（用于同步等需要“当前时间”的场景）。
#[inline]
pub fn system_time_to_prost_timestamp() -> Timestamp {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "system clock is before UNIX_EPOCH; using zero timestamp");
            std::time::Duration::ZERO
        });
    Timestamp {
        seconds: t.as_secs() as i64,
        nanos: t.subsec_nanos() as i32,
    }
}
