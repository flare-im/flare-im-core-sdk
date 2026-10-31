//! 服务端时钟估计：本机时钟与服务端时钟的偏差。
//!
//! 服务端签发的截止时间（插件目录的 `validUntilMs`、实时包的 `expires_at_ms`、令牌的 `exp`）
//! 都是服务端时钟；拿本机时钟去比，设备时钟快就提前失效、慢就迟迟不过期。这里从长连接下行帧
//! 自带的服务端时间戳估出偏差，凡是拿本机时间比服务端截止时间的地方都改问 [`ServerClock`]。
//!
//! 每个样本是一个下界：帧在服务端打上时间戳之后还要走一段网络才到本机，所以
//! `服务端时间戳 - 本机收到时刻 <= 真实偏差`。取最近一段时间内样本的最大值，误差是最短的那次
//! 单程延迟。窗口外的样本丢掉（至少保留最新一个），本机时钟被调过之后不会一直沿用旧偏差。
//! 偏差按本机墙钟记，设备休眠期间墙钟照走，醒来后估计仍然成立。
//!
//! 还没有样本（尚未连上）时偏差为 0，即退回本机时钟。

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::shared::util::now_millis;

/// 早于这个时刻（2020-01-01）的时间戳不当样本：没填的、或填错的帧。
const MIN_PLAUSIBLE_SERVER_MS: u64 = 1_577_836_800_000;
/// 样本参与估计的时间窗。
const SAMPLE_WINDOW_MS: i64 = 120_000;
/// 最多保留的样本数。
const MAX_SAMPLES: usize = 16;

#[derive(Clone, Copy, Debug)]
struct Sample {
    /// 服务端时间戳减本机收到时刻。
    offset_ms: i64,
    /// 本机收到时刻（墙钟）。
    local_ms: i64,
}

/// 本机与服务端时钟偏差的估计，多处共享同一份（克隆共享）。
#[derive(Clone, Default)]
pub struct ServerClock {
    samples: Arc<Mutex<VecDeque<Sample>>>,
}

impl ServerClock {
    pub fn new() -> Self {
        Self::default()
    }

    /// 记一个服务端时间戳（毫秒），视为此刻收到。
    pub fn observe_server_ms(&self, server_ms: u64) {
        self.observe_at(server_ms, now_millis());
    }

    fn observe_at(&self, server_ms: u64, local_ms: u64) {
        if server_ms < MIN_PLAUSIBLE_SERVER_MS {
            return;
        }
        let (Ok(server), Ok(local)) = (i64::try_from(server_ms), i64::try_from(local_ms)) else {
            return;
        };
        let Ok(mut samples) = self.samples.lock() else {
            return;
        };
        samples.push_back(Sample {
            offset_ms: server - local,
            local_ms: local,
        });
        while samples.len() > MAX_SAMPLES
            || (samples.len() > 1
                && samples
                    .front()
                    .is_some_and(|oldest| (local - oldest.local_ms).abs() > SAMPLE_WINDOW_MS))
        {
            samples.pop_front();
        }
    }

    /// 服务端时钟减本机时钟的估计（毫秒）；没有样本时为 0。
    pub fn offset_ms(&self) -> i64 {
        self.samples
            .lock()
            .ok()
            .and_then(|samples| samples.iter().map(|sample| sample.offset_ms).max())
            .unwrap_or(0)
    }

    /// 此刻的服务端时间估计（毫秒）。
    pub fn now_ms(&self) -> i64 {
        i64::try_from(now_millis())
            .unwrap_or(i64::MAX)
            .saturating_add(self.offset_ms())
    }

    /// 此刻的服务端时间估计（毫秒，无符号）。
    pub fn now_millis(&self) -> u64 {
        u64::try_from(self.now_ms().max(0)).unwrap_or_default()
    }

    /// 此刻的服务端时间估计（秒）。
    pub fn now_secs(&self) -> u64 {
        self.now_millis() / 1000
    }

    /// 服务端时刻换成本机时钟上的同一时刻：服务端签发的截止时间交给只认本机时钟的比较之前用。
    pub fn to_local_ms(&self, server_ms: i64) -> i64 {
        server_ms.saturating_sub(self.offset_ms())
    }

    /// [`Self::to_local_ms`] 的无符号版本。
    pub fn to_local_millis(&self, server_ms: u64) -> u64 {
        let server = i64::try_from(server_ms).unwrap_or(i64::MAX);
        u64::try_from(self.to_local_ms(server).max(0)).unwrap_or_default()
    }
}

impl std::fmt::Debug for ServerClock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerClock")
            .field("offset_ms", &self.offset_ms())
            .finish()
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::ServerClock;

    /// 设备时钟比服务端快 `ahead_ms`（负数为慢）时的时钟：按此刻收到一个服务端时间戳。
    pub(crate) fn clock_with_device_ahead_by(ahead_ms: i64) -> ServerClock {
        let clock = ServerClock::new();
        let local = crate::shared::util::now_millis() as i64;
        clock.observe_server_ms((local - ahead_ms) as u64);
        clock
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCAL: u64 = 1_800_000_000_000;

    #[test]
    fn without_samples_the_device_clock_is_used() {
        let clock = ServerClock::new();
        assert_eq!(clock.offset_ms(), 0);
        assert_eq!(clock.to_local_ms(LOCAL as i64), LOCAL as i64);
    }

    #[test]
    fn a_device_clock_ahead_gives_a_negative_offset() {
        let clock = ServerClock::new();
        clock.observe_at(LOCAL - 5 * 60_000, LOCAL);
        assert_eq!(clock.offset_ms(), -5 * 60_000);
        assert_eq!(
            clock.to_local_ms((LOCAL - 5 * 60_000 + 30_000) as i64),
            (LOCAL + 30_000) as i64
        );
    }

    #[test]
    fn a_device_clock_behind_gives_a_positive_offset() {
        let clock = ServerClock::new();
        clock.observe_at(LOCAL + 7 * 60_000, LOCAL);
        assert_eq!(clock.offset_ms(), 7 * 60_000);
    }

    #[test]
    fn the_least_delayed_sample_wins() {
        let clock = ServerClock::new();
        // Same true offset (-1000), seen through 300 ms, 20 ms and 900 ms of delivery delay.
        clock.observe_at(LOCAL - 1_300, LOCAL);
        clock.observe_at(LOCAL + 1_000 - 1_020, LOCAL + 1_000);
        clock.observe_at(LOCAL + 2_000 - 1_900, LOCAL + 2_000);
        assert_eq!(clock.offset_ms(), -1_020);
    }

    #[test]
    fn samples_outside_the_window_stop_counting() {
        let clock = ServerClock::new();
        clock.observe_at(LOCAL + 60_000, LOCAL);
        // The device clock was moved forward by ten minutes: the true offset drops.
        let later = LOCAL + 10 * 60_000;
        clock.observe_at(later - 600_000 + 60_000, later);
        assert_eq!(clock.offset_ms(), 60_000 - 600_000);
    }

    #[test]
    fn the_latest_sample_is_kept_however_old() {
        let clock = ServerClock::new();
        clock.observe_at(LOCAL + 4_000, LOCAL);
        assert_eq!(clock.offset_ms(), 4_000);
    }

    #[test]
    fn implausible_timestamps_are_ignored() {
        let clock = ServerClock::new();
        clock.observe_at(0, LOCAL);
        clock.observe_at(42, LOCAL);
        assert_eq!(clock.offset_ms(), 0);
    }

    #[test]
    fn clones_share_the_estimate() {
        let clock = ServerClock::new();
        let shared = clock.clone();
        clock.observe_at(LOCAL - 2_000, LOCAL);
        assert_eq!(shared.offset_ms(), -2_000);
    }
}
