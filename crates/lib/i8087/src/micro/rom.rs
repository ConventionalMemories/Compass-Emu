//! The 8087's microcode ROM: 1648 words of 16 bits.
//!
//! Compass-Emu does not ship the ROM. It is read from a file the user supplies, in the text
//! form the "Opcode Collective" dump uses (`8087mc.txt` in github.com/a-mcego/granite,
//! tools/8087mc/bin): one line per word, 16 columns for the bits A..P (A = bit 15), a column
//! holding its letter when the bit is 1 and a blank when it is 0; anything after column 16
//! is a comment. A plain binary file of 3296 bytes (big-endian words) is accepted too.

pub const WORDS: usize = 1648;

#[derive(Clone)]
pub struct MicroRom(pub Box<[u16; WORDS]>);

impl MicroRom {
    /// Parses the letter form or the binary form.
    pub fn parse(data: &[u8]) -> Result<MicroRom, String> {
        let mut rom = Box::new([0u16; WORDS]);
        if data.len() == WORDS * 2 {
            for (i, w) in rom.iter_mut().enumerate() {
                *w = u16::from_be_bytes([data[2 * i], data[2 * i + 1]]);
            }
            return Ok(MicroRom(rom));
        }
        let text = std::str::from_utf8(data).map_err(|_| "8087 microcode: not text and not 3296 bytes".to_string())?;
        let mut n = 0;
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            if n == WORDS {
                return Err("8087 microcode: more than 1648 lines".into());
            }
            let b = line.as_bytes();
            let mut w = 0u16;
            for i in 0..16 {
                w <<= 1;
                match b.get(i) {
                    Some(&c) if c == b'A' + i as u8 => w |= 1,
                    Some(b' ') | None => {}
                    Some(_) => return Err(format!("8087 microcode: line {} column {} is not a bit letter", n + 1, i + 1)),
                }
            }
            rom[n] = w;
            n += 1;
        }
        if n != WORDS {
            return Err(format!("8087 microcode: {} lines, expected 1648", n));
        }
        // every routine starts from "read next instruction" at address 0
        if rom[0] != 0xFFFE {
            return Err("8087 microcode: word 0 is not the end-of-instruction word".into());
        }
        Ok(MicroRom(rom))
    }

    pub fn load(path: &std::path::Path) -> Result<MicroRom, String> {
        let data = std::fs::read(path).map_err(|e| format!("{}: {}", path.display(), e))?;
        MicroRom::parse(&data)
    }

    /// The ROM named by the I8087_MICROCODE environment variable, or `roms/8087mc.txt` /
    /// `roms/8087mc.bin` under the current directory or beside the program. None if there is none.
    pub fn find() -> Option<MicroRom> {
        if let Ok(p) = std::env::var("I8087_MICROCODE") {
            return MicroRom::load(std::path::Path::new(&p)).ok();
        }
        let exe = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.to_path_buf()));
        for dir in [Some(std::path::PathBuf::from(".")), exe].into_iter().flatten() {
            for p in ["roms/8087mc.txt", "roms/8087mc.bin"] {
                if let Ok(r) = MicroRom::load(&dir.join(p)) {
                    return Some(r);
                }
            }
        }
        None
    }
}
