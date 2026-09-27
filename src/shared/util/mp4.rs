//! MP4 / MOV / M4A（ISO-BMFF）容器时长探测。
//!
//! 只走顶层 box 找到 `moov`，再在它下面找 `mvhd`，按
//! `duration * 1000 / timescale` 换成毫秒。`mdat` 等大 box 直接 seek 跳过，
//! 整个过程只读十几个字节的头；读不出来（webm、wav、截断、垃圾）一律返回 `None`，
//! 不报错 —— 调用方拿它补时长，补不上就维持原样。

use std::io::{Read, Seek, SeekFrom};

/// 顶层 box 数量上限：正常文件只有 ftyp/free/mdat/moov/udta 这么几个，
/// 超过说明是恰好能解析成小 box 的垃圾数据，别在上面一直 seek。
const MAX_TOP_LEVEL_BOXES: usize = 64;
/// `moov` 子 box 上限（`mvhd` 按规范在最前，多轨也就几十个 `trak`）。
const MAX_MOOV_CHILDREN: usize = 256;

struct BoxHeader {
    kind: [u8; 4],
    body_start: u64,
    end: u64,
}

/// 读容器时长（毫秒）。不是可解析的 ISO-BMFF、没有 `mvhd` 或时长为 0/未知时返回 `None`。
pub fn duration_ms<R: Read + Seek>(reader: &mut R) -> Option<u64> {
    let len = reader.seek(SeekFrom::End(0)).ok()?;
    let mut pos = 0;
    for _ in 0..MAX_TOP_LEVEL_BOXES {
        let header = read_box_header(reader, pos, len)?;
        if &header.kind == b"moov" {
            return movie_header_duration_ms(reader, &header);
        }
        pos = header.end;
    }
    None
}

/// 按路径读本地文件的容器时长；打不开或读不出返回 `None`。
#[cfg(not(target_arch = "wasm32"))]
pub fn file_duration_ms(path: &std::path::Path) -> Option<u64> {
    let mut file = std::fs::File::open(path).ok()?;
    duration_ms(&mut file)
}

fn movie_header_duration_ms<R: Read + Seek>(reader: &mut R, moov: &BoxHeader) -> Option<u64> {
    let mut pos = moov.body_start;
    for _ in 0..MAX_MOOV_CHILDREN {
        let child = read_box_header(reader, pos, moov.end)?;
        if &child.kind == b"mvhd" {
            return parse_movie_header(reader, &child);
        }
        pos = child.end;
    }
    None
}

/// `mvhd` 是 full box：version(1) flags(3)，之后
/// v0 = creation(4) modification(4) timescale(4) duration(4)；
/// v1 = creation(8) modification(8) timescale(4) duration(8)。
/// 时长全 1 表示「未知」。
fn parse_movie_header<R: Read + Seek>(reader: &mut R, mvhd: &BoxHeader) -> Option<u64> {
    let available = mvhd.end - mvhd.body_start;
    reader.seek(SeekFrom::Start(mvhd.body_start)).ok()?;
    let mut version_flags = [0u8; 4];
    reader.read_exact(&mut version_flags).ok()?;
    let (timescale, duration) = match version_flags[0] {
        0 => {
            let mut fields = [0u8; 16];
            if available < 4 + fields.len() as u64 {
                return None;
            }
            reader.read_exact(&mut fields).ok()?;
            let duration = u32::from_be_bytes(fields[12..16].try_into().ok()?);
            if duration == u32::MAX {
                return None;
            }
            (
                u32::from_be_bytes(fields[8..12].try_into().ok()?),
                u64::from(duration),
            )
        }
        1 => {
            let mut fields = [0u8; 28];
            if available < 4 + fields.len() as u64 {
                return None;
            }
            reader.read_exact(&mut fields).ok()?;
            let duration = u64::from_be_bytes(fields[20..28].try_into().ok()?);
            if duration == u64::MAX {
                return None;
            }
            (
                u32::from_be_bytes(fields[16..20].try_into().ok()?),
                duration,
            )
        }
        _ => return None,
    };
    if timescale == 0 || duration == 0 {
        return None;
    }
    let millis = u128::from(duration) * 1000 / u128::from(timescale);
    u64::try_from(millis).ok().filter(|millis| *millis > 0)
}

/// 读 `start` 处的 box 头。size==1 取后面 8 字节的 largesize，size==0 表示延伸到 `limit`；
/// box 越出 `limit`（截断）或类型不是可打印 ASCII（多半不是 ISO-BMFF）时返回 `None`。
fn read_box_header<R: Read + Seek>(reader: &mut R, start: u64, limit: u64) -> Option<BoxHeader> {
    if limit.checked_sub(start)? < 8 {
        return None;
    }
    reader.seek(SeekFrom::Start(start)).ok()?;
    let mut head = [0u8; 8];
    reader.read_exact(&mut head).ok()?;
    let kind: [u8; 4] = head[4..8].try_into().ok()?;
    if !kind.iter().all(|b| b.is_ascii_graphic() || *b == b' ') {
        return None;
    }
    let (size, header_len) = match u32::from_be_bytes(head[0..4].try_into().ok()?) {
        0 => (limit - start, 8),
        1 => {
            let mut large = [0u8; 8];
            reader.read_exact(&mut large).ok()?;
            (u64::from_be_bytes(large), 16)
        }
        size => (u64::from(size), 8),
    };
    if size < header_len {
        return None;
    }
    let end = start.checked_add(size)?;
    if end > limit {
        return None;
    }
    Some(BoxHeader {
        kind,
        body_start: start + header_len,
        end,
    })
}

/// 测试用的极简 MP4：`ftyp` + `moov{mvhd v0}` + 一段 `mdat`。
#[cfg(test)]
pub(crate) fn tiny_mp4(timescale: u32, duration: u32) -> Vec<u8> {
    let mut mvhd = vec![0, 0, 0, 0];
    mvhd.extend_from_slice(&[0; 8]);
    mvhd.extend_from_slice(&timescale.to_be_bytes());
    mvhd.extend_from_slice(&duration.to_be_bytes());
    mvhd.extend_from_slice(&[0; 80]);
    let mut out = mp4_box(b"ftyp", b"isom\0\0\x02\0isomiso2mp41");
    out.extend(mp4_box(b"moov", &mp4_box(b"mvhd", &mvhd)));
    out.extend(mp4_box(b"mdat", &[0; 64]));
    out
}

#[cfg(test)]
fn mp4_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    out
}

#[cfg(test)]
mod tests {
    use super::{duration_ms, mp4_box, tiny_mp4};
    use std::io::{Cursor, Read, Seek, SeekFrom};

    fn mvhd_v1(timescale: u32, duration: u64) -> Vec<u8> {
        let mut body = vec![1, 0, 0, 0];
        body.extend_from_slice(&[0; 16]);
        body.extend_from_slice(&timescale.to_be_bytes());
        body.extend_from_slice(&duration.to_be_bytes());
        body.extend_from_slice(&[0; 80]);
        mp4_box(b"mvhd", &body)
    }

    fn probe(bytes: &[u8]) -> Option<u64> {
        duration_ms(&mut Cursor::new(bytes))
    }

    /// 记下实际读了多少字节，用来证明 `mdat` 是跳过去的而不是读过去的。
    struct CountingReader<R> {
        inner: R,
        read: usize,
    }

    impl<R: Read> Read for CountingReader<R> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = self.inner.read(buf)?;
            self.read += n;
            Ok(n)
        }
    }

    impl<R: Seek> Seek for CountingReader<R> {
        fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
            self.inner.seek(pos)
        }
    }

    #[test]
    fn version_0_movie_header_gives_milliseconds() {
        assert_eq!(probe(&tiny_mp4(1000, 4000)), Some(4000));
        // 常见的 600 / 90000 时基
        assert_eq!(probe(&tiny_mp4(600, 2400)), Some(4000));
        assert_eq!(probe(&tiny_mp4(90_000, 373_500)), Some(4150));
    }

    #[test]
    fn version_1_movie_header_gives_milliseconds() {
        let mut bytes = mp4_box(b"ftyp", b"qt  \0\0\0\0qt  ");
        bytes.extend(mp4_box(b"moov", &mvhd_v1(90_000, 360_000)));
        assert_eq!(probe(&bytes), Some(4000));

        // 64 位时长：10 小时，44.1kHz 时基
        let mut long = mp4_box(b"ftyp", b"M4A \0\0\0\0M4A ");
        long.extend(mp4_box(b"moov", &mvhd_v1(44_100, 44_100 * 36_000)));
        assert_eq!(probe(&long), Some(36_000_000));
    }

    #[test]
    fn moov_after_a_large_mdat_is_found_without_reading_the_media() {
        let mut bytes = mp4_box(b"ftyp", b"isom\0\0\x02\0isom");
        bytes.extend(mp4_box(b"mdat", &vec![0xAB; 1 << 20]));
        bytes.extend(mp4_box(b"moov", &mvhd_v1(1000, 4000)));
        let mut reader = CountingReader {
            inner: Cursor::new(bytes),
            read: 0,
        };
        assert_eq!(duration_ms(&mut reader), Some(4000));
        assert!(
            reader.read < 256,
            "should seek past mdat, but read {} bytes",
            reader.read
        );
    }

    #[test]
    fn a_64_bit_box_size_is_followed() {
        let payload = [0u8; 40];
        let mut bytes = mp4_box(b"ftyp", b"isom\0\0\x02\0isom");
        bytes.extend_from_slice(&1u32.to_be_bytes());
        bytes.extend_from_slice(b"mdat");
        bytes.extend_from_slice(&((16 + payload.len()) as u64).to_be_bytes());
        bytes.extend_from_slice(&payload);
        bytes.extend(mp4_box(b"moov", &mvhd_v1(1000, 4000)));
        assert_eq!(probe(&bytes), Some(4000));
    }

    #[test]
    fn a_zero_size_moov_runs_to_the_end_of_the_file() {
        let mut bytes = mp4_box(b"ftyp", b"isom\0\0\x02\0isom");
        bytes.extend(mp4_box(b"mdat", &[0; 32]));
        let moov_at = bytes.len();
        bytes.extend(mp4_box(b"moov", &mvhd_v1(1000, 4000)));
        bytes[moov_at..moov_at + 4].copy_from_slice(&0u32.to_be_bytes());
        assert_eq!(probe(&bytes), Some(4000));
    }

    #[test]
    fn mvhd_is_found_behind_other_moov_children() {
        let mut moov = mp4_box(b"iods", &[0; 16]);
        moov.extend(mvhd_v1(1000, 1500));
        let mut bytes = mp4_box(b"ftyp", b"isom\0\0\x02\0isom");
        bytes.extend(mp4_box(b"moov", &moov));
        assert_eq!(probe(&bytes), Some(1500));
    }

    #[test]
    fn anything_that_is_not_a_parsable_mp4_gives_nothing() {
        // 空文件、太短
        assert_eq!(probe(&[]), None);
        assert_eq!(probe(&[0, 0, 0]), None);
        // webm（EBML 头）
        assert_eq!(
            probe(&[
                0x1A, 0x45, 0xDF, 0xA3, 0x9F, 0x42, 0x86, 0x81, 0x01, 0x42, 0xF7, 0x81, 0x01
            ]),
            None
        );
        // wav
        let mut wav = b"RIFF".to_vec();
        wav.extend_from_slice(&36u32.to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&[0; 40]);
        assert_eq!(probe(&wav), None);
        // 文本垃圾
        assert_eq!(probe(b"hello, this is definitely not a video file"), None);
        // 没有 moov
        assert_eq!(probe(&mp4_box(b"ftyp", b"isom\0\0\x02\0isom")), None);
        // box 声明的长度小于头长度
        let mut bad = 4u32.to_be_bytes().to_vec();
        bad.extend_from_slice(b"ftyp");
        assert_eq!(probe(&bad), None);
    }

    #[test]
    fn a_truncated_file_gives_nothing() {
        let full = tiny_mp4(1000, 4000);
        // 截在 moov 中间
        assert_eq!(probe(&full[..40]), None);
        // mdat 在前、声明的长度超过实际文件（录制中断）
        let mut cut = mp4_box(b"ftyp", b"isom\0\0\x02\0isom");
        cut.extend(mp4_box(b"mdat", &[0; 64]));
        cut.truncate(cut.len() - 10);
        assert_eq!(probe(&cut), None);
    }

    #[test]
    fn zero_unknown_or_unscaled_durations_give_nothing() {
        assert_eq!(probe(&tiny_mp4(1000, 0)), None);
        assert_eq!(probe(&tiny_mp4(0, 4000)), None);
        assert_eq!(probe(&tiny_mp4(1000, u32::MAX)), None);
        let mut v1_unknown = mp4_box(b"ftyp", b"isom\0\0\x02\0isom");
        v1_unknown.extend(mp4_box(b"moov", &mvhd_v1(1000, u64::MAX)));
        assert_eq!(probe(&v1_unknown), None);
        // 未知 version
        let mut v2 = mp4_box(b"ftyp", b"isom\0\0\x02\0isom");
        let mut body = vec![2, 0, 0, 0];
        body.extend_from_slice(&[0; 40]);
        v2.extend(mp4_box(b"moov", &mp4_box(b"mvhd", &body)));
        assert_eq!(probe(&v2), None);
        // mvhd 被截短（声明长度够不到 duration 字段）
        let mut short = mp4_box(b"ftyp", b"isom\0\0\x02\0isom");
        short.extend(mp4_box(
            b"moov",
            &mp4_box(b"mvhd", &[0, 0, 0, 0, 0, 0, 0, 0]),
        ));
        assert_eq!(probe(&short), None);
    }
}
