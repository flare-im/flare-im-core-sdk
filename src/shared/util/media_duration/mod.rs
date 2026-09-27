//! 本地音视频文件的时长探测（毫秒）。
//!
//! 宿主按本地路径发音视频时通常不带时长；发送前用它从文件头读出来补上。
//! 按魔数分派：`RIFF` 走 WAV，其余交给 MP4/MOV/M4A（ISO-BMFF）。
//! 只读容器头、不整文件读入；认不出或读不出一律 `None`，从不报错。

pub mod mp4;
pub mod wav;

use std::io::{Read, Seek, SeekFrom};

/// 读容器时长（毫秒）；不是认得的容器、或时长为 0/未知时返回 `None`。
pub fn duration_ms<R: Read + Seek>(reader: &mut R) -> Option<u64> {
    reader.seek(SeekFrom::Start(0)).ok()?;
    let mut magic = [0u8; 4];
    reader.read_exact(&mut magic).ok()?;
    if &magic == b"RIFF" {
        wav::duration_ms(reader)
    } else {
        mp4::duration_ms(reader)
    }
}

/// 按路径读本地文件的时长；打不开或读不出返回 `None`。
#[cfg(not(target_arch = "wasm32"))]
pub fn file_duration_ms(path: &std::path::Path) -> Option<u64> {
    let mut file = std::fs::File::open(path).ok()?;
    duration_ms(&mut file)
}

#[cfg(test)]
mod tests {
    use super::{duration_ms, mp4::tiny_mp4, wav::tiny_wav};
    use std::io::Cursor;

    #[test]
    fn dispatches_by_magic() {
        assert_eq!(
            duration_ms(&mut Cursor::new(tiny_mp4(1000, 4000))),
            Some(4000)
        );
        assert_eq!(
            duration_ms(&mut Cursor::new(tiny_wav(32_000, 96_000))),
            Some(3000)
        );
        assert_eq!(duration_ms(&mut Cursor::new(b"RIF".to_vec())), None);
        assert_eq!(duration_ms(&mut Cursor::new(Vec::new())), None);
    }
}
