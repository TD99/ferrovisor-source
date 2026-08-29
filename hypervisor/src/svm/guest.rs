use core::arch::global_asm;

global_asm!(include_str!("tiny_guest.S"));

unsafe extern "C" {
    static tiny_guest_start: u8;
    static tiny_guest_end: u8;
}

pub fn image() -> &'static [u8] {
    unsafe {
        let start = &tiny_guest_start as *const u8;
        let end = &tiny_guest_end as *const u8;
        core::slice::from_raw_parts(start, end.offset_from(start) as usize)
    }
}

