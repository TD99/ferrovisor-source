use crate::arch;

pub fn read_ascii_blocking() -> u8 {
    let mut shift = false;
    loop {
        while unsafe { arch::inb(0x64) } & 1 == 0 {
            core::hint::spin_loop();
        }
        let code = unsafe { arch::inb(0x60) };
        match code {
            0x2A | 0x36 => { shift = true; continue; }
            0xAA | 0xB6 => { shift = false; continue; }
            code if code & 0x80 != 0 => continue,
            _ => {}
        }
        if let Some(byte) = map_set1(code, shift) {
            return byte;
        }
    }
}

fn map_set1(code: u8, shift: bool) -> Option<u8> {
    let normal = match code {
        0x02..=0x0B => b"1234567890"[(code - 0x02) as usize],
        0x10..=0x19 => b"qwertyuiop"[(code - 0x10) as usize],
        0x1E..=0x26 => b"asdfghjkl"[(code - 0x1E) as usize],
        0x2C..=0x32 => b"zxcvbnm"[(code - 0x2C) as usize],
        0x39 => b' ',
        0x1C => b'\n',
        0x0E => 8,
        0x0C => b'-',
        0x0D => b'=',
        0x1A => b'[',
        0x1B => b']',
        0x27 => b';',
        0x28 => b'\'',
        0x33 => b',',
        0x34 => b'.',
        0x35 => b'/',
        _ => return None,
    };
    Some(if shift && normal.is_ascii_lowercase() {
        normal.to_ascii_uppercase()
    } else {
        normal
    })
}

