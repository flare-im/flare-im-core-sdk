use rand::Rng;
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU64, Ordering};

use super::time;

static SEQ: AtomicU64 = AtomicU64::new(0);

pub fn generate_client_msg_id() -> String {
    // 1. 高熵源组合
    let ts = time::now_millis();

    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let random = rand::thread_rng().r#gen::<u128>();

    // 2. 拼接唯一熵源（时间戳 + 自增序列 + 安全随机）
    let entropy = format!("{ts}-{seq}-{random}");

    // 3. SHA256 哈希（飞书标准算法）
    let mut hasher = Sha256::new();
    hasher.update(entropy.as_bytes());
    let hash = hasher.finalize();

    // 4. 转32位小写十六进制（取16字节 = 32个十六进制字符）
    hex::encode(&hash[..16])
}

pub fn now_millis() -> u64 {
    time::now_millis()
}
