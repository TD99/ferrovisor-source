use core::arch::asm;

pub const EFER: u32 = 0xC000_0080;
pub const VM_CR: u32 = 0xC001_0114;
pub const VM_HSAVE_PA: u32 = 0xC001_0117;

#[inline]
pub unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    unsafe { asm!("in al, dx", out("al") value, in("dx") port, options(nomem, nostack)) };
    value
}

#[inline]
pub unsafe fn outb(port: u16, value: u8) {
    unsafe { asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack)) };
}

#[inline]
pub unsafe fn rdmsr(msr: u32) -> u64 {
    let low: u32;
    let high: u32;
    unsafe { asm!("rdmsr", in("ecx") msr, out("eax") low, out("edx") high) };
    ((high as u64) << 32) | low as u64
}

#[inline]
pub unsafe fn wrmsr(msr: u32, value: u64) {
    unsafe {
        asm!(
            "wrmsr",
            in("ecx") msr,
            in("eax") value as u32,
            in("edx") (value >> 32) as u32,
        )
    };
}

pub fn disable_interrupts() {
    unsafe { asm!("cli", options(nomem, nostack)) };
}

pub fn halt_forever() -> ! {
    disable_interrupts();
    loop {
        unsafe { asm!("hlt", options(nomem, nostack)) };
    }
}

