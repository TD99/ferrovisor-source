use super::{GuestMemory, allocate_guest_range, allocate_zeroed};
use crate::memory::{FrameAllocator, PAGE_SIZE};

const GPA_UEFI_SYSTEM_TABLE: u64 = 0x10_0000;
const GPA_UEFI_INPUT: u64 = 0x10_1000;
const GPA_UEFI_OUTPUT: u64 = 0x10_2000;
const GPA_UEFI_MODE: u64 = 0x10_3000;
const GPA_UEFI_VENDOR: u64 = 0x10_4000;
const GPA_UEFI_STUBS: u64 = 0x10_5000;
const GPA_IMAGE: u64 = 0x20_0000;
const GPA_STACK: u64 = 0x1000_000;
const STACK_SIZE: u64 = 256 * 1024;
const MAX_IMAGE_SIZE: u64 = GPA_STACK - GPA_IMAGE;

const TRAMPOLINE_OFFSET: u64 = 0x00;
const RESET_OFFSET: u64 = 0x20;
const OUTPUT_STRING_OFFSET: u64 = 0x30;
const NOOP_OFFSET: u64 = 0x50;
const CLEAR_SCREEN_OFFSET: u64 = 0x60;
const READ_KEY_OFFSET: u64 = 0x70;
const SET_ATTRIBUTE_OFFSET: u64 = 0x90;
const SET_CURSOR_OFFSET: u64 = 0xA0;

pub(super) struct UefiLaunch {
    pub entry: u64,
    pub trampoline: u64,
    pub stack_pointer: u64,
}

pub(super) fn prepare(
    frames: &mut FrameAllocator<'_>,
    physical_offset: u64,
    guest_memory: &mut GuestMemory,
    image: &[u8],
) -> Result<UefiLaunch, &'static str> {
    let entry = load_pe(frames, physical_offset, guest_memory, image)?;
    allocate_guest_range(frames, physical_offset, guest_memory, GPA_STACK, STACK_SIZE)?;
    for gpa in [
        GPA_UEFI_SYSTEM_TABLE,
        GPA_UEFI_INPUT,
        GPA_UEFI_OUTPUT,
        GPA_UEFI_MODE,
        GPA_UEFI_VENDOR,
        GPA_UEFI_STUBS,
    ] {
        guest_memory.add(gpa, allocate_zeroed(frames, physical_offset)?)?;
    }
    install_console_protocols(guest_memory, physical_offset)?;
    let trampoline = GPA_UEFI_STUBS + TRAMPOLINE_OFFSET;
    write_trampoline(guest_memory, physical_offset, trampoline, entry)?;
    Ok(UefiLaunch {
        entry,
        trampoline,
        stack_pointer: GPA_STACK + STACK_SIZE,
    })
}

struct PeLayout {
    entry_rva: u32,
    image_base: u64,
    image_size: u32,
    header_size: u32,
    relocation_rva: u32,
    relocation_size: u32,
    section_table: usize,
    section_count: usize,
}

fn load_pe(
    frames: &mut FrameAllocator<'_>,
    physical_offset: u64,
    guest_memory: &mut GuestMemory,
    image: &[u8],
) -> Result<u64, &'static str> {
    let pe = parse_pe(image)?;
    let image_size = pe.image_size as u64;
    if image_size == 0 || image_size > MAX_IMAGE_SIZE {
        return Err("external UEFI image is too large for the guest address space");
    }
    if pe.header_size == 0 || pe.header_size as u64 > image_size {
        return Err("PE headers exceed the image size");
    }
    allocate_guest_range(frames, physical_offset, guest_memory, GPA_IMAGE, image_size)?;
    guest_memory.copy_to(
        physical_offset,
        GPA_IMAGE,
        slice_at(image, 0, pe.header_size as usize)?,
    )?;

    for section_index in 0..pe.section_count {
        let section_offset = pe
            .section_table
            .checked_add(
                section_index
                    .checked_mul(40)
                    .ok_or("PE section table overflow")?,
            )
            .ok_or("PE section table overflow")?;
        let section = slice_at(image, section_offset, 40)?;
        let virtual_size = u32_at(section, 8)? as u64;
        let virtual_address = u32_at(section, 12)? as u64;
        let raw_size = u32_at(section, 16)? as usize;
        let raw_offset = u32_at(section, 20)? as usize;
        let mapped_size = virtual_size.max(raw_size as u64);
        let section_end = virtual_address
            .checked_add(mapped_size)
            .ok_or("PE section exceeds the image")?;
        if section_end > image_size {
            return Err("PE section exceeds the image");
        }
        if raw_size != 0 {
            guest_memory.copy_to(
                physical_offset,
                GPA_IMAGE
                    .checked_add(virtual_address)
                    .ok_or("PE section address overflow")?,
                slice_at(image, raw_offset, raw_size)?,
            )?;
        }
    }

    apply_relocations(
        guest_memory,
        physical_offset,
        GPA_IMAGE,
        pe.image_base,
        pe.relocation_rva,
        pe.relocation_size,
        image_size,
    )?;
    GPA_IMAGE
        .checked_add(pe.entry_rva as u64)
        .ok_or("PE entry point overflow")
}

fn parse_pe(image: &[u8]) -> Result<PeLayout, &'static str> {
    if u16_at(image, 0)? != 0x5A4D {
        return Err("external image is not a PE/COFF executable");
    }
    let pe_offset = u32_at(image, 0x3C)? as usize;
    if slice_at(image, pe_offset, 4)? != b"PE\0\0" {
        return Err("PE signature is missing");
    }
    let file_header = slice_at(image, pe_offset + 4, 20)?;
    if u16_at(file_header, 0)? != 0x8664 {
        return Err("external UEFI image is not x86_64");
    }
    let section_count = u16_at(file_header, 2)? as usize;
    let optional_size = u16_at(file_header, 16)? as usize;
    let optional = slice_at(image, pe_offset + 24, optional_size)?;
    if u16_at(optional, 0)? != 0x20B {
        return Err("external UEFI image is not PE32+");
    }
    let section_alignment = u32_at(optional, 32)?;
    if section_alignment != PAGE_SIZE as u32 {
        return Err("external UEFI image does not use 4 KiB sections");
    }
    let image_size = u32_at(optional, 56)?;
    let header_size = u32_at(optional, 60)?;
    let directory_count = u32_at(optional, 108)?;
    if directory_count <= 5 {
        return Err("external UEFI image has no relocation directory");
    }
    let relocation_directory = 112 + 5 * 8;
    let relocation_rva = u32_at(optional, relocation_directory)?;
    let relocation_size = u32_at(optional, relocation_directory + 4)?;
    if relocation_rva == 0 || relocation_size == 0 {
        return Err("external UEFI image has no base relocations");
    }
    let section_table = pe_offset
        .checked_add(24)
        .and_then(|offset| offset.checked_add(optional_size))
        .ok_or("PE section table overflow")?;
    let section_bytes = section_count
        .checked_mul(40)
        .ok_or("PE section table overflow")?;
    slice_at(image, section_table, section_bytes)?;
    let entry_rva = u32_at(optional, 16)?;
    if entry_rva >= image_size {
        return Err("PE entry point is outside the image");
    }
    Ok(PeLayout {
        entry_rva,
        image_base: u64_at(optional, 24)?,
        image_size,
        header_size,
        relocation_rva,
        relocation_size,
        section_table,
        section_count,
    })
}

fn apply_relocations(
    guest_memory: &GuestMemory,
    physical_offset: u64,
    load_base: u64,
    image_base: u64,
    relocation_rva: u32,
    relocation_size: u32,
    image_size: u64,
) -> Result<(), &'static str> {
    let delta = load_base.wrapping_sub(image_base);
    if delta == 0 {
        return Ok(());
    }
    let mut cursor = relocation_rva as u64;
    let end = cursor
        .checked_add(relocation_size as u64)
        .ok_or("PE relocation directory overflow")?;
    if end > image_size {
        return Err("PE relocation directory exceeds the image");
    }
    while cursor < end {
        if end - cursor < 8 {
            return Err("PE relocation block is truncated");
        }
        let block_rva = read_image_u32(guest_memory, physical_offset, load_base, cursor)?;
        let block_size = read_image_u32(guest_memory, physical_offset, load_base, cursor + 4)?;
        if block_size < 8 || cursor + block_size as u64 > end {
            return Err("PE relocation block is invalid");
        }
        let entry_count = (block_size as u64 - 8) / 2;
        for entry_index in 0..entry_count {
            let entry = read_image_u16(
                guest_memory,
                physical_offset,
                load_base,
                cursor + 8 + entry_index * 2,
            )?;
            let kind = entry >> 12;
            let offset = (entry & 0x0FFF) as u64;
            if kind == 0 {
                continue;
            }
            if kind != 10 {
                return Err("external UEFI image uses an unsupported relocation type");
            }
            let target = (block_rva as u64)
                .checked_add(offset)
                .ok_or("PE relocation target overflow")?;
            if target
                .checked_add(8)
                .ok_or("PE relocation target overflow")?
                > image_size
            {
                return Err("PE relocation target exceeds the image");
            }
            let value = read_image_u64(guest_memory, physical_offset, load_base, target)?
                .wrapping_add(delta);
            write_image_u64(guest_memory, physical_offset, load_base, target, value)?;
        }
        cursor += block_size as u64;
    }
    Ok(())
}

fn read_image_u16(
    guest_memory: &GuestMemory,
    physical_offset: u64,
    image_base: u64,
    offset: u64,
) -> Result<u16, &'static str> {
    let mut bytes = [0; 2];
    guest_memory.copy_from(
        physical_offset,
        image_base
            .checked_add(offset)
            .ok_or("guest image address overflow")?,
        &mut bytes,
    )?;
    Ok(u16::from_le_bytes(bytes))
}

fn read_image_u32(
    guest_memory: &GuestMemory,
    physical_offset: u64,
    image_base: u64,
    offset: u64,
) -> Result<u32, &'static str> {
    let mut bytes = [0; 4];
    guest_memory.copy_from(
        physical_offset,
        image_base
            .checked_add(offset)
            .ok_or("guest image address overflow")?,
        &mut bytes,
    )?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_image_u64(
    guest_memory: &GuestMemory,
    physical_offset: u64,
    image_base: u64,
    offset: u64,
) -> Result<u64, &'static str> {
    let mut bytes = [0; 8];
    guest_memory.copy_from(
        physical_offset,
        image_base
            .checked_add(offset)
            .ok_or("guest image address overflow")?,
        &mut bytes,
    )?;
    Ok(u64::from_le_bytes(bytes))
}

fn write_image_u64(
    guest_memory: &GuestMemory,
    physical_offset: u64,
    image_base: u64,
    offset: u64,
    value: u64,
) -> Result<(), &'static str> {
    guest_memory.copy_to(
        physical_offset,
        image_base
            .checked_add(offset)
            .ok_or("guest image address overflow")?,
        &value.to_le_bytes(),
    )
}

fn install_console_protocols(
    guest_memory: &GuestMemory,
    physical_offset: u64,
) -> Result<(), &'static str> {
    for (index, byte) in b"Ferrovisor UEFI".iter().enumerate() {
        write_u16(
            guest_memory,
            physical_offset,
            GPA_UEFI_VENDOR,
            (index * 2) as u64,
            *byte as u16,
        )?;
    }

    write_u64(
        guest_memory,
        physical_offset,
        GPA_UEFI_INPUT,
        0x00,
        stub(RESET_OFFSET),
    )?;
    write_u64(
        guest_memory,
        physical_offset,
        GPA_UEFI_INPUT,
        0x08,
        stub(READ_KEY_OFFSET),
    )?;
    write_u64(guest_memory, physical_offset, GPA_UEFI_INPUT, 0x10, 0)?;

    for (offset, target) in [
        (0x00, RESET_OFFSET),
        (0x08, OUTPUT_STRING_OFFSET),
        (0x10, NOOP_OFFSET),
        (0x18, NOOP_OFFSET),
        (0x20, NOOP_OFFSET),
        (0x28, SET_ATTRIBUTE_OFFSET),
        (0x30, CLEAR_SCREEN_OFFSET),
        (0x38, SET_CURSOR_OFFSET),
        (0x40, NOOP_OFFSET),
    ] {
        write_u64(
            guest_memory,
            physical_offset,
            GPA_UEFI_OUTPUT,
            offset,
            stub(target),
        )?;
    }
    write_u64(
        guest_memory,
        physical_offset,
        GPA_UEFI_OUTPUT,
        0x48,
        GPA_UEFI_MODE,
    )?;

    write_i32(guest_memory, physical_offset, GPA_UEFI_MODE, 0x00, 1)?;
    write_i32(guest_memory, physical_offset, GPA_UEFI_MODE, 0x04, 0)?;
    write_i32(guest_memory, physical_offset, GPA_UEFI_MODE, 0x08, 7)?;
    write_i32(guest_memory, physical_offset, GPA_UEFI_MODE, 0x0C, 0)?;
    write_i32(guest_memory, physical_offset, GPA_UEFI_MODE, 0x10, 0)?;
    write_i32(guest_memory, physical_offset, GPA_UEFI_MODE, 0x14, 1)?;

    write_u64(
        guest_memory,
        physical_offset,
        GPA_UEFI_SYSTEM_TABLE,
        0x00,
        0x5453_5953_2049_4249,
    )?;
    write_u32(
        guest_memory,
        physical_offset,
        GPA_UEFI_SYSTEM_TABLE,
        0x08,
        0x0002_0070,
    )?;
    write_u32(
        guest_memory,
        physical_offset,
        GPA_UEFI_SYSTEM_TABLE,
        0x0C,
        120,
    )?;
    write_u32(
        guest_memory,
        physical_offset,
        GPA_UEFI_SYSTEM_TABLE,
        0x10,
        0,
    )?;
    write_u32(
        guest_memory,
        physical_offset,
        GPA_UEFI_SYSTEM_TABLE,
        0x14,
        0,
    )?;
    write_u64(
        guest_memory,
        physical_offset,
        GPA_UEFI_SYSTEM_TABLE,
        0x18,
        GPA_UEFI_VENDOR,
    )?;
    write_u32(
        guest_memory,
        physical_offset,
        GPA_UEFI_SYSTEM_TABLE,
        0x20,
        1,
    )?;
    write_u64(
        guest_memory,
        physical_offset,
        GPA_UEFI_SYSTEM_TABLE,
        0x28,
        0,
    )?;
    write_u64(
        guest_memory,
        physical_offset,
        GPA_UEFI_SYSTEM_TABLE,
        0x30,
        GPA_UEFI_INPUT,
    )?;
    write_u64(
        guest_memory,
        physical_offset,
        GPA_UEFI_SYSTEM_TABLE,
        0x38,
        0,
    )?;
    write_u64(
        guest_memory,
        physical_offset,
        GPA_UEFI_SYSTEM_TABLE,
        0x40,
        GPA_UEFI_OUTPUT,
    )?;
    write_u64(
        guest_memory,
        physical_offset,
        GPA_UEFI_SYSTEM_TABLE,
        0x48,
        0,
    )?;
    write_u64(
        guest_memory,
        physical_offset,
        GPA_UEFI_SYSTEM_TABLE,
        0x50,
        GPA_UEFI_OUTPUT,
    )?;
    write_u64(
        guest_memory,
        physical_offset,
        GPA_UEFI_SYSTEM_TABLE,
        0x58,
        0,
    )?;
    write_u64(
        guest_memory,
        physical_offset,
        GPA_UEFI_SYSTEM_TABLE,
        0x60,
        0,
    )?;
    write_u64(
        guest_memory,
        physical_offset,
        GPA_UEFI_SYSTEM_TABLE,
        0x68,
        0,
    )?;
    write_u64(
        guest_memory,
        physical_offset,
        GPA_UEFI_SYSTEM_TABLE,
        0x70,
        0,
    )?;

    write_stub_code(guest_memory, physical_offset)
}

fn write_stub_code(guest_memory: &GuestMemory, physical_offset: u64) -> Result<(), &'static str> {
    guest_memory.copy_to(physical_offset, stub(RESET_OFFSET), &[0x31, 0xC0, 0xC3])?;
    guest_memory.copy_to(
        physical_offset,
        stub(OUTPUT_STRING_OFFSET),
        &[
            0x49, 0x89, 0xD0, // mov r8, rdx
            0x41, 0x0F, 0xB7, 0x00, // movzx eax, word ptr [r8]
            0x49, 0x83, 0xC0, 0x02, // add r8, 2
            0x66, 0x85, 0xC0, // test ax, ax
            0x74, 0x07, // jz done
            0x66, 0xBA, 0xF8, 0x03, // mov dx, 0x3f8
            0xEE, // out dx, al
            0xEB, 0xEC, // jmp next
            0x31, 0xC0, 0xC3, // done: xor eax, eax; ret
        ],
    )?;
    guest_memory.copy_to(physical_offset, stub(NOOP_OFFSET), &[0x31, 0xC0, 0xC3])?;
    guest_memory.copy_to(
        physical_offset,
        stub(CLEAR_SCREEN_OFFSET),
        &[0x66, 0xBA, 0xF8, 0x03, 0xB0, 0x0C, 0xEE, 0x31, 0xC0, 0xC3],
    )?;
    guest_memory.copy_to(
        physical_offset,
        stub(READ_KEY_OFFSET),
        &[
            0x49, 0x89, 0xD0, // mov r8, rdx
            0x31, 0xC0, // xor eax, eax
            0x66, 0xBA, 0xF8, 0x03, // mov dx, 0x3f8
            0xEC, // in al, dx
            0x66, 0x41, 0xC7, 0x00, 0x00, 0x00, // mov word ptr [r8], 0
            0x66, 0x41, 0x89, 0x40, 0x02, // mov word ptr [r8 + 2], ax
            0x31, 0xC0, 0xC3, // xor eax, eax; ret
        ],
    )?;
    guest_memory.copy_to(
        physical_offset,
        stub(SET_ATTRIBUTE_OFFSET),
        &[
            0x89, 0xD0, // mov eax, edx
            0x66, 0xBA, 0xFB, 0x03, // mov dx, 0x3fb
            0xEE, // out dx, al
            0x31, 0xC0, 0xC3, // xor eax, eax; ret
        ],
    )?;
    guest_memory.copy_to(
        physical_offset,
        stub(SET_CURSOR_OFFSET),
        &[
            0x49, 0x89, 0xD1, // mov r9, rdx
            0x66, 0xBA, 0xF9, 0x03, // mov dx, 0x3f9
            0x44, 0x89, 0xC8, // mov eax, r9d
            0xEE, // out dx, al
            0x66, 0xBA, 0xFA, 0x03, // mov dx, 0x3fa
            0x44, 0x89, 0xC0, // mov eax, r8d
            0xEE, // out dx, al
            0x31, 0xC0, 0xC3, // xor eax, eax; ret
        ],
    )
}

fn write_trampoline(
    guest_memory: &GuestMemory,
    physical_offset: u64,
    trampoline: u64,
    entry: u64,
) -> Result<(), &'static str> {
    let mut code = [0u8; 25];
    code[0..2].copy_from_slice(&[0x31, 0xC9]); // xor ecx, ecx
    code[2..4].copy_from_slice(&[0x48, 0xBA]);
    code[4..12].copy_from_slice(&GPA_UEFI_SYSTEM_TABLE.to_le_bytes());
    code[12..14].copy_from_slice(&[0x48, 0xB8]);
    code[14..22].copy_from_slice(&entry.to_le_bytes());
    code[22..24].copy_from_slice(&[0xFF, 0xD0]); // call rax
    code[24] = 0xF4; // hlt after return
    guest_memory.copy_to(physical_offset, trampoline, &code)
}

fn stub(offset: u64) -> u64 {
    GPA_UEFI_STUBS + offset
}

fn write_u64(
    guest_memory: &GuestMemory,
    physical_offset: u64,
    address: u64,
    offset: u64,
    value: u64,
) -> Result<(), &'static str> {
    guest_memory.copy_to(
        physical_offset,
        address
            .checked_add(offset)
            .ok_or("UEFI table address overflow")?,
        &value.to_le_bytes(),
    )
}

fn write_u32(
    guest_memory: &GuestMemory,
    physical_offset: u64,
    address: u64,
    offset: u64,
    value: u32,
) -> Result<(), &'static str> {
    guest_memory.copy_to(
        physical_offset,
        address
            .checked_add(offset)
            .ok_or("UEFI table address overflow")?,
        &value.to_le_bytes(),
    )
}

fn write_u16(
    guest_memory: &GuestMemory,
    physical_offset: u64,
    address: u64,
    offset: u64,
    value: u16,
) -> Result<(), &'static str> {
    guest_memory.copy_to(
        physical_offset,
        address
            .checked_add(offset)
            .ok_or("UEFI table address overflow")?,
        &value.to_le_bytes(),
    )
}

fn write_i32(
    guest_memory: &GuestMemory,
    physical_offset: u64,
    address: u64,
    offset: u64,
    value: i32,
) -> Result<(), &'static str> {
    write_u32(guest_memory, physical_offset, address, offset, value as u32)
}

fn slice_at(bytes: &[u8], offset: usize, length: usize) -> Result<&[u8], &'static str> {
    let end = offset
        .checked_add(length)
        .ok_or("external image is truncated")?;
    bytes.get(offset..end).ok_or("external image is truncated")
}

fn u16_at(bytes: &[u8], offset: usize) -> Result<u16, &'static str> {
    Ok(u16::from_le_bytes(
        slice_at(bytes, offset, 2)?
            .try_into()
            .map_err(|_| "truncated integer")?,
    ))
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, &'static str> {
    Ok(u32::from_le_bytes(
        slice_at(bytes, offset, 4)?
            .try_into()
            .map_err(|_| "truncated integer")?,
    ))
}

fn u64_at(bytes: &[u8], offset: usize) -> Result<u64, &'static str> {
    Ok(u64::from_le_bytes(
        slice_at(bytes, offset, 8)?
            .try_into()
            .map_err(|_| "truncated integer")?,
    ))
}
