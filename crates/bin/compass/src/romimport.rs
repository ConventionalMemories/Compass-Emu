//! Finding the Compass ROMs in a folder or zip file (MAME sets, own dumps) by their CRC-32
//! and copying them into a ROM folder under the names the emulator looks for.

use std::path::{Path, PathBuf};

/// (CRC-32, size, file name in the ROM folder, what it is)
const KNOWN: &[(u32, usize, &str, &str)] = &[
    (0xbd7a385a, 8192, "1101even.rev3.bin", "1101 boot PROM rev 3 (even)"),
    (0x751c893e, 8192, "1101odd.rev3.bin", "1101 boot PROM rev 3 (odd)"),
    (0x051b11e5, 8192, "1101even.rev4.bin", "1101 boot PROM rev 4 (even)"),
    (0x898d7ce2, 8192, "1101odd.rev4.bin", "1101 boot PROM rev 4 (odd)"),
    (0x0ede3636, 8192, "1129even.ccprom.bin", "1129 boot PROM (even)"),
    (0x07b29b05, 8192, "1129odd.ccprom.bin", "1129 boot PROM (odd)"),
    (0x67071849, 8192, "1139even.ccprom.bin", "1139 boot PROM (even)"),
    (0x13ed4bf0, 8192, "1139odd.ccprom.bin", "1139 boot PROM (odd)"),
    (0x48cb0b57, 1024, "300067-01.bin", "keyboard firmware 300067-01"),
    (0xcebbccdc, 1024, "300001-02.bin", "keyboard firmware 300001-02 (1137)"),
    (0x6ea21f23, 4096, "300237-01.bin", "2102 floppy firmware 300237-01"),
    (0x85850e38, 4096, "300237-02.bin", "2102 floppy firmware 300237-02"),
    (0x769c3bab, 8192, "300205-05.bin", "2101 firmware 300205-05 (20 MB box)"),
    (0x34e191f5, 8192, "300205-06.bin", "2101 firmware 300205-06 (10 MB box)"),
];

fn known(d: &[u8]) -> Option<&'static (u32, usize, &'static str, &'static str)> {
    let crc = crc32fast::hash(d);
    KNOWN.iter().find(|k| k.0 == crc && k.1 == d.len())
}

/// The known ROMs in one file's bytes: the file itself, or a 16K boot PROM dump with the
/// even and odd bytes interleaved (split into its two halves).
fn identify(name: &str, d: &[u8], out: &mut Vec<(String, Vec<u8>, String)>) {
    if let Some(k) = known(d) {
        out.push((k.2.to_string(), d.to_vec(), format!("{} ({name})", k.3)));
        return;
    }
    if d.len() == 16384 {
        let even: Vec<u8> = d.iter().step_by(2).copied().collect();
        let odd: Vec<u8> = d.iter().skip(1).step_by(2).copied().collect();
        if let (Some(e), Some(o)) = (known(&even), known(&odd)) {
            out.push((e.2.to_string(), even, format!("{} ({name}, split)", e.3)));
            out.push((o.2.to_string(), odd, format!("{} ({name}, split)", o.3)));
            return;
        }
    }
    // The 8087 microcode ROM (a user-supplied dump, text or binary) goes by its name.
    let lower = name.to_ascii_lowercase();
    let base = lower.rsplit(['/', '\\']).next().unwrap_or(&lower);
    if base == "8087mc.txt" || base == "8087mc.bin" {
        out.push((base.to_string(), d.to_vec(), format!("8087 microcode ROM ({name})")));
    }
}

fn scan_zip(path: &Path, out: &mut Vec<(String, Vec<u8>, String)>) -> Result<(), String> {
    let f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| format!("{}: {e}", path.display()))?;
    for i in 0..z.len() {
        let mut e = z.by_index(i).map_err(|e| format!("{}: {e}", path.display()))?;
        if !e.is_file() || e.size() > 1 << 20 {
            continue;
        }
        let name = format!("{}:{}", path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), e.name());
        let mut d = Vec::new();
        if std::io::Read::read_to_end(&mut e, &mut d).is_ok() {
            identify(&name, &d, out);
        }
    }
    Ok(())
}

fn scan(path: &Path, depth: u32, out: &mut Vec<(String, Vec<u8>, String)>, errs: &mut Vec<String>) {
    if path.is_dir() {
        if depth > 3 {
            return;
        }
        if let Ok(rd) = std::fs::read_dir(path) {
            for e in rd.flatten() {
                scan(&e.path(), depth + 1, out, errs);
            }
        }
        return;
    }
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if ext == "zip" {
        if let Err(e) = scan_zip(path, out) {
            errs.push(e);
        }
        return;
    }
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let lower = name.to_ascii_lowercase();
    if size <= 16384 || lower == "8087mc.txt" || lower == "8087mc.bin" {
        if let Ok(d) = std::fs::read(path) {
            identify(&name, &d, out);
        }
    }
}

/// Look through `src` (a folder, searched three levels deep, or a zip file) and copy every
/// known ROM into `dest`. Returns one line per ROM copied, then any problems.
pub fn import(src: &Path, dest: &Path) -> Vec<String> {
    let mut found = vec![];
    let mut errs = vec![];
    scan(src, 0, &mut found, &mut errs);
    let mut log = vec![];
    if let Err(e) = std::fs::create_dir_all(dest) {
        return vec![format!("cannot make {}: {e}", dest.display())];
    }
    let mut done: Vec<String> = vec![];
    for (file, data, what) in found {
        if done.contains(&file) {
            continue;
        }
        let to: PathBuf = dest.join(&file);
        if std::fs::read(&to).is_ok_and(|d| d == data) {
            log.push(format!("already there: {what}"));
        }
        else {
            match std::fs::write(&to, &data) {
                Ok(_) => log.push(format!("copied {what} as {file}")),
                Err(e) => log.push(format!("cannot write {}: {e}", to.display())),
            }
        }
        done.push(file);
    }
    if done.is_empty() {
        log.push(format!("no known Compass ROMs found in {}", src.display()));
    }
    log.extend(errs);
    log
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_names_are_unique_and_match_lookup() {
        for (i, k) in KNOWN.iter().enumerate() {
            assert!(KNOWN.iter().skip(i + 1).all(|o| o.2 != k.2), "{}", k.2);
        }
        let mut out = vec![];
        identify("x", &[1, 2, 3], &mut out);
        assert!(out.is_empty());
        identify("dir/8087MC.TXT", b"ABCD", &mut out);
        assert_eq!(out[0].0, "8087mc.txt");
    }
}
