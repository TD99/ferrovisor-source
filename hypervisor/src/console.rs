use crate::{arch, svm::keyboard};
use core::{
    fmt::{self, Write},
    ptr::{read_volatile, write_volatile},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

const VGA_WIDTH: usize = 80;
const VGA_HEIGHT: usize = 25;
const VGA_PHYSICAL: u64 = 0xB8000;

static VGA_ADDRESS: AtomicU64 = AtomicU64::new(0);
static LOCK: AtomicBool = AtomicBool::new(false);
static mut COLUMN: usize = 0;
static mut ROW: usize = 0;

pub fn init(physical_offset: u64) {
    VGA_ADDRESS.store(physical_offset + VGA_PHYSICAL, Ordering::Relaxed);
    serial_init();
    clear();
}

pub fn clear() {
    let _guard = ConsoleGuard::lock();
    unsafe { clear_unlocked() };
}

pub fn show_startup_screen() {
    let _guard = ConsoleGuard::lock();
    unsafe {
        clear_unlocked();
        write_colored_text_unlocked("\n  +--------------------------------------------------------------------------+\n", 0x0B);
        write_colored_text_unlocked("  |                            F E R R O V I S O R                           |\n", 0x0A);
        write_colored_text_unlocked("  |                  AMD-V / SVM Type-1 Hypervisor                           |\n", 0x07);
        write_colored_text_unlocked("  +--------------------------------------------------------------------------+\n\n", 0x0B);
    }
}

unsafe fn clear_unlocked() {
    let base = VGA_ADDRESS.load(Ordering::Relaxed) as *mut u16;
    for index in 0..VGA_WIDTH * VGA_HEIGHT {
        unsafe { write_volatile(base.add(index), 0x0720) };
    }
    unsafe {
        ROW = 0;
        COLUMN = 0;
    }
}

fn serial_init() {
    unsafe {
        arch::outb(0x3F9, 0x00);
        arch::outb(0x3FB, 0x80);
        arch::outb(0x3F8, 0x01);
        arch::outb(0x3F9, 0x00);
        arch::outb(0x3FB, 0x03);
        arch::outb(0x3FA, 0xC7);
        arch::outb(0x3FC, 0x0B);
    }
}

unsafe fn serial_write(byte: u8) {
    while unsafe { arch::inb(0x3FD) } & 0x20 == 0 {
        core::hint::spin_loop();
    }
    unsafe { arch::outb(0x3F8, byte) };
}

pub fn write_guest_byte(byte: u8) {
    let _guard = ConsoleGuard::lock();
    unsafe { write_byte_unlocked(byte) };
}

pub fn read_guest_byte_blocking() -> u8 {
    loop {
        if unsafe { arch::inb(0x3FD) } & 1 != 0 {
            return unsafe { arch::inb(0x3F8) };
        }
        if let Some(byte) = keyboard::try_read_ascii() {
            return byte;
        }
        core::hint::spin_loop();
    }
}

unsafe fn write_byte_unlocked(byte: u8) {
    unsafe { write_colored_byte_unlocked(byte, 0x0F) };
}

unsafe fn write_colored_text_unlocked(text: &str, color: u8) {
    for byte in text.bytes() {
        unsafe { write_colored_byte_unlocked(byte, color) };
    }
}

unsafe fn write_colored_byte_unlocked(byte: u8, color: u8) {
    if byte == 0x0C {
        unsafe { clear_unlocked() };
        return;
    }

    if byte == b'\n' {
        unsafe { serial_write(b'\r') };
    }
    unsafe { serial_write(byte) };
    match byte {
        b'\n' => unsafe {
            COLUMN = 0;
            ROW += 1;
        },
        8 => unsafe {
            if COLUMN > 0 {
                COLUMN -= 1;
                let cell = (VGA_ADDRESS.load(Ordering::Relaxed) as *mut u16)
                    .add(ROW * VGA_WIDTH + COLUMN);
                write_volatile(cell, 0x0720);
            }
        },
        byte if byte.is_ascii_graphic() || byte == b' ' => unsafe {
            let cell = (VGA_ADDRESS.load(Ordering::Relaxed) as *mut u16)
                .add(ROW * VGA_WIDTH + COLUMN);
            write_volatile(cell, ((color as u16) << 8) | byte as u16);
            COLUMN += 1;
            if COLUMN == VGA_WIDTH {
                COLUMN = 0;
                ROW += 1;
            }
        },
        _ => {}
    }
    unsafe { scroll_if_needed() };
}

unsafe fn scroll_if_needed() {
    unsafe {
        if ROW < VGA_HEIGHT {
            return;
        }
        let base = VGA_ADDRESS.load(Ordering::Relaxed) as *mut u16;
        for row in 1..VGA_HEIGHT {
            for col in 0..VGA_WIDTH {
                let value = read_volatile(base.add(row * VGA_WIDTH + col));
                write_volatile(base.add((row - 1) * VGA_WIDTH + col), value);
            }
        }
        for col in 0..VGA_WIDTH {
            write_volatile(base.add((VGA_HEIGHT - 1) * VGA_WIDTH + col), 0x0720);
        }
        ROW = VGA_HEIGHT - 1;
    }
}

struct ConsoleGuard;

impl ConsoleGuard {
    fn lock() -> Self {
        while LOCK
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        Self
    }
}

impl Drop for ConsoleGuard {
    fn drop(&mut self) {
        LOCK.store(false, Ordering::Release);
    }
}

struct ConsoleWriter;

impl Write for ConsoleWriter {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let _guard = ConsoleGuard::lock();
        for byte in text.bytes() {
            unsafe { write_byte_unlocked(byte) };
        }
        Ok(())
    }
}

pub fn _print(args: fmt::Arguments) {
    let _ = ConsoleWriter.write_fmt(args);
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::console::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($fmt:expr) => ($crate::print!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => ($crate::print!(concat!($fmt, "\n"), $($arg)*));
}
