//! ImageDisk (.IMD) floppy files: turned into the raw .img the drives use.
//!
//! An IMD file is a text header ending in 1Ah, then one record per track: mode, cylinder,
//! head, sector count, size code, the sector ID map (plus optional cylinder and head maps),
//! then each sector as a type byte and its data (types 2, 4, 6, 8: one byte filled over the
//! whole sector; 0: no data). The raw image is the tracks in cylinder, head order with each
//! track's sectors sorted by ID, which suits both GRiD 2102 disks (side 1 numbered 10-18) and
//! IBM-style ones (both sides 1-9).

use std::path::{Path, PathBuf};

/// True when the file name ends in .imd (any case).
pub fn is_imd(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("imd"))
}

/// Raw image bytes from IMD file bytes.
pub fn to_img(d: &[u8]) -> Result<Vec<u8>, String> {
    if !d.starts_with(b"IMD") {
        return Err("not an ImageDisk file (no IMD header)".into());
    }
    let mut p = d.iter().position(|&b| b == 0x1A).ok_or("ImageDisk header has no end (1Ah)")? + 1;
    let mut tracks: Vec<(u8, u8, Vec<(u8, Vec<u8>)>)> = vec![];
    let short = || "ImageDisk file ends in the middle of a track".to_string();
    while p < d.len() {
        let hdr = d.get(p..p + 5).ok_or_else(short)?;
        let (cyl, head, ns, sz) = (hdr[1], hdr[2], hdr[3] as usize, hdr[4]);
        p += 5;
        if sz > 6 {
            return Err(format!("ImageDisk track {cyl}/{}: sector size code {sz} is not supported", head & 1));
        }
        let n = 128usize << sz;
        let ids = d.get(p..p + ns).ok_or_else(short)?.to_vec();
        p += ns;
        if head & 0x80 != 0 {
            p += ns;
        }
        if head & 0x40 != 0 {
            p += ns;
        }
        let mut secs = Vec::with_capacity(ns);
        for &id in &ids {
            let t = *d.get(p).ok_or_else(short)?;
            p += 1;
            let data = match t {
                0 => vec![0u8; n],
                1 | 3 | 5 | 7 => {
                    let s = d.get(p..p + n).ok_or_else(short)?.to_vec();
                    p += n;
                    s
                }
                2 | 4 | 6 | 8 => {
                    let b = *d.get(p).ok_or_else(short)?;
                    p += 1;
                    vec![b; n]
                }
                _ => return Err(format!("ImageDisk track {cyl}/{}: unknown sector type {t}", head & 1)),
            };
            secs.push((id, data));
        }
        secs.sort_by_key(|s| s.0);
        tracks.push((cyl, head & 1, secs));
    }
    tracks.sort_by_key(|t| (t.0, t.1));
    Ok(tracks.into_iter().flat_map(|t| t.2.into_iter().flat_map(|s| s.1)).collect())
}

/// The raw image to use for a floppy file: the path itself, or for an .IMD the .img with the
/// same name beside it, made from the IMD when it is not there yet. The emulator writes
/// changes back to the .img, so the IMD stays as it was.
pub fn floppy_path(path: &Path) -> Result<(PathBuf, Option<String>), String> {
    if !is_imd(path) {
        return Ok((path.to_path_buf(), None));
    }
    let img = path.with_extension("img");
    if img.exists() {
        return Ok((img.clone(), Some(format!("using {} (the raw image beside the .IMD)", img.display()))));
    }
    let d = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let raw = to_img(&d).map_err(|e| format!("{}: {e}", path.display()))?;
    std::fs::write(&img, &raw).map_err(|e| format!("cannot write {}: {e}", img.display()))?;
    Ok((img.clone(), Some(format!("made {} ({} KB) from the .IMD", img.display(), raw.len() / 1024))))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A GRiD-style IMD (side 1 IDs 10-18, interleaved, some filled sectors) comes back as
    /// the image it was made from.
    #[test]
    fn grid_imd_round_trip() {
        const INTER: [u8; 9] = [3, 5, 7, 9, 2, 4, 6, 8, 1];
        let img: Vec<u8> = (0..40 * 2 * 9 * 512).map(|i| if (i / 512) % 3 == 0 { 0xE5 } else { (i * 7 / 3) as u8 }).collect();
        let mut f = b"IMD 1.18: test\r\ncomment\x1a".to_vec();
        for c in 0..40u8 {
            for h in 0..2u8 {
                f.extend([5, c, h, 9, 2]);
                f.extend(INTER.iter().map(|s| s + 9 * h));
                for s in INTER {
                    let lin = (c as usize * 2 + h as usize) * 9 + (s as usize - 1);
                    let data = &img[lin * 512..(lin + 1) * 512];
                    if data.iter().all(|&b| b == data[0]) {
                        f.extend([2, data[0]]);
                    }
                    else {
                        f.push(1);
                        f.extend(data);
                    }
                }
            }
        }
        assert_eq!(to_img(&f).unwrap(), img);
        assert!(to_img(b"nope").is_err());
        assert!(to_img(&f[..f.len() - 3]).is_err());
    }
}
