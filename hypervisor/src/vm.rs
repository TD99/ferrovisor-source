use core::arch::global_asm;
use crate::external::{ExternalUefiImage, EXTERNAL_UEFI_IMAGES};

#[derive(Clone, Copy)]
pub struct VmImage {
    pub name: &'static str,
    pub description: &'static str,
    pub image: &'static [u8],
}

pub fn available() -> [VmImage; 1] {
    [tiny64()]
}

pub fn tiny64() -> VmImage {
    VmImage {
        name: "Tiny64",
        description: "Interactive 64-bit demo OS",
        image: tiny64::image(),
    }
}

pub fn external_uefi() -> &'static [ExternalUefiImage] {
    EXTERNAL_UEFI_IMAGES
}

mod tiny64 {
    use super::global_asm;

    global_asm!(include_str!("../../guests/tiny64/tiny64.S"));

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
}
