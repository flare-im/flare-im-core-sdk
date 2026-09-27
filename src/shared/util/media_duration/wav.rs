//! WAV（RIFF/WAVE）时长探测。
//!
//! `fmt ` 取 byte_rate，`data` 取块大小，按 `data_size * 1000 / byte_rate` 换成毫秒。
//! 录音中途或流式写入的文件，`data` 大小常是 0xFFFFFFFF 或还没回填准：越过文件末尾时
//! 按实际剩余字节算。`WAVE_FORMAT_EXTENSIBLE` 同样只看 byte_rate。RF64、非 RIFF、
//! 截断在 `fmt `/`data` 之前的文件一律 `None`。

use std::io::{Read, Seek, SeekFrom};

/// chunk 数量上限：正常文件就 fmt/LIST/fact/data 几个，防止在垃圾数据上一直 seek。
const MAX_CHUNKS: usize = 64;
/// 流式写入时 `data` 大小的占位值。
const UNKNOWN_DATA_SIZE: u32 = u32::MAX;

/// 读 WAV 时长（毫秒）。不是 RIFF/WAVE、缺 `fmt `/`data`、byte_rate 为 0 或没有音频数据时返回 `None`。
pub fn duration_ms<R: Read + Seek>(reader: &mut R) -> Option<u64> {
    let len = reader.seek(SeekFrom::End(0)).ok()?;
    reader.seek(SeekFrom::Start(0)).ok()?;
    let mut header = [0u8; 12];
    reader.read_exact(&mut header).ok()?;
    if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        return None;
    }

    let mut byte_rate = None;
    let mut data_size = None;
    let mut pos = header.len() as u64;
    for _ in 0..MAX_CHUNKS {
        if byte_rate.is_some() && data_size.is_some() {
            break;
        }
        if len.saturating_sub(pos) < 8 {
            break;
        }
        reader.seek(SeekFrom::Start(pos)).ok()?;
        let mut chunk = [0u8; 8];
        reader.read_exact(&mut chunk).ok()?;
        let size = u32::from_le_bytes(chunk[4..8].try_into().ok()?);
        let body = pos + 8;
        let remaining = len - body;
        match &chunk[0..4] {
            b"fmt " => {
                if size < 16 || u64::from(size) > remaining {
                    return None;
                }
                // format(2) channels(2) sample_rate(4) byte_rate(4) block_align(2) bits(2)
                let mut fmt = [0u8; 16];
                reader.read_exact(&mut fmt).ok()?;
                byte_rate = Some(u32::from_le_bytes(fmt[8..12].try_into().ok()?));
            }
            b"data" => {
                let declared = u64::from(size);
                data_size = Some(if size == UNKNOWN_DATA_SIZE || declared > remaining {
                    remaining
                } else {
                    declared
                });
            }
            _ => {}
        }
        // chunk 按偶数对齐：奇数长度后面跟一个 pad 字节。
        pos = body + u64::from(size) + u64::from(size & 1);
    }

    let byte_rate = byte_rate.filter(|rate| *rate > 0)?;
    let data_size = data_size.filter(|size| *size > 0)?;
    let millis = u128::from(data_size) * 1000 / u128::from(byte_rate);
    u64::try_from(millis).ok().filter(|millis| *millis > 0)
}

/// 测试用的 PCM WAV：单声道 16 位，采样率 = byte_rate / 2，`data_len` 字节静音。
#[cfg(test)]
pub(crate) fn tiny_wav(byte_rate: u32, data_len: u32) -> Vec<u8> {
    let mut body = b"WAVE".to_vec();
    body.extend(wav_chunk(b"fmt ", &pcm_fmt(byte_rate)));
    body.extend(wav_chunk(b"data", &vec![0; data_len as usize]));
    riff(&body)
}

#[cfg(test)]
fn pcm_fmt(byte_rate: u32) -> Vec<u8> {
    let mut fmt = 1u16.to_le_bytes().to_vec(); // WAVE_FORMAT_PCM
    fmt.extend_from_slice(&1u16.to_le_bytes()); // channels
    fmt.extend_from_slice(&(byte_rate / 2).to_le_bytes()); // sample_rate
    fmt.extend_from_slice(&byte_rate.to_le_bytes());
    fmt.extend_from_slice(&2u16.to_le_bytes()); // block_align
    fmt.extend_from_slice(&16u16.to_le_bytes()); // bits_per_sample
    fmt
}

#[cfg(test)]
fn wav_chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = id.to_vec();
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(body);
    if body.len() % 2 == 1 {
        out.push(0);
    }
    out
}

#[cfg(test)]
fn riff(body: &[u8]) -> Vec<u8> {
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(body);
    out
}

#[cfg(test)]
mod tests {
    use super::{duration_ms, pcm_fmt, riff, tiny_wav, wav_chunk};
    use std::io::Cursor;

    fn probe(bytes: &[u8]) -> Option<u64> {
        duration_ms(&mut Cursor::new(bytes))
    }

    /// `fmt ` + `data`，`data` 头里声明的大小可以和实际字节数不同。
    fn wav_with_declared_data(byte_rate: u32, declared: u32, actual: usize) -> Vec<u8> {
        let mut body = b"WAVE".to_vec();
        body.extend(wav_chunk(b"fmt ", &pcm_fmt(byte_rate)));
        body.extend_from_slice(b"data");
        body.extend_from_slice(&declared.to_le_bytes());
        body.extend(vec![0; actual]);
        riff(&body)
    }

    #[test]
    fn a_pcm_wav_gives_milliseconds() {
        // Flutter kit 语音：16kHz 单声道 pcm16 = 32000 B/s
        assert_eq!(probe(&tiny_wav(32_000, 96_000)), Some(3000));
        assert_eq!(probe(&tiny_wav(32_000, 40_000)), Some(1250));
        // 44.1kHz 立体声 16 位
        assert_eq!(probe(&tiny_wav(176_400, 352_800)), Some(2000));
    }

    #[test]
    fn wave_format_extensible_only_needs_the_byte_rate() {
        let mut fmt = 0xFFFEu16.to_le_bytes().to_vec(); // WAVE_FORMAT_EXTENSIBLE
        fmt.extend_from_slice(&pcm_fmt(32_000)[2..]);
        fmt.extend_from_slice(&22u16.to_le_bytes()); // cbSize
        fmt.extend_from_slice(&[0; 22]); // valid bits / channel mask / sub-format GUID
        let mut body = b"WAVE".to_vec();
        body.extend(wav_chunk(b"fmt ", &fmt));
        body.extend(wav_chunk(b"data", &[0; 64_000]));
        assert_eq!(probe(&riff(&body)), Some(2000));
    }

    #[test]
    fn odd_sized_chunks_are_followed_past_their_pad_byte() {
        let mut body = b"WAVE".to_vec();
        body.extend(wav_chunk(b"fmt ", &pcm_fmt(32_000)));
        body.extend(wav_chunk(b"LIST", b"INFOx")); // 5 字节 + 1 pad
        body.extend(wav_chunk(b"fact", &[1, 2, 3])); // 3 字节 + 1 pad
        body.extend(wav_chunk(b"data", &[0; 32_000]));
        assert_eq!(probe(&riff(&body)), Some(1000));
    }

    #[test]
    fn a_data_size_past_the_end_uses_the_bytes_actually_there() {
        // 录音中途：头里写的是最终大小，文件里只有 2 秒
        assert_eq!(
            probe(&wav_with_declared_data(32_000, 1_000_000, 64_000)),
            Some(2000)
        );
        // 流式写入的占位大小
        assert_eq!(
            probe(&wav_with_declared_data(32_000, u32::MAX, 48_000)),
            Some(1500)
        );
    }

    #[test]
    fn a_zero_byte_rate_or_empty_data_gives_nothing() {
        assert_eq!(probe(&tiny_wav(0, 32_000)), None);
        assert_eq!(probe(&tiny_wav(32_000, 0)), None);
    }

    #[test]
    fn a_file_cut_before_its_fmt_or_data_gives_nothing() {
        let full = tiny_wav(32_000, 96_000);
        assert_eq!(probe(&full[..10]), None); // RIFF 头都不全
        assert_eq!(probe(&full[..20]), None); // 截在 fmt 中间
        assert_eq!(probe(&full[..36]), None); // fmt 完整、没有 data
        // fmt 太短
        let mut body = b"WAVE".to_vec();
        body.extend(wav_chunk(b"fmt ", &pcm_fmt(32_000)[..12]));
        body.extend(wav_chunk(b"data", &[0; 32_000]));
        assert_eq!(probe(&riff(&body)), None);
    }

    #[test]
    fn anything_that_is_not_riff_wave_gives_nothing() {
        assert_eq!(probe(&[]), None);
        let mut rf64 = tiny_wav(32_000, 32_000);
        rf64[0..4].copy_from_slice(b"RF64");
        assert_eq!(probe(&rf64), None);
        let mut rifx = tiny_wav(32_000, 32_000);
        rifx[0..4].copy_from_slice(b"RIFX");
        assert_eq!(probe(&rifx), None);
        let mut avi = tiny_wav(32_000, 32_000);
        avi[8..12].copy_from_slice(b"AVI ");
        assert_eq!(probe(&avi), None);
        assert_eq!(probe(&super::super::mp4::tiny_mp4(1000, 4000)), None);
        assert_eq!(probe(b"hello, this is definitely not audio"), None);
    }
}
