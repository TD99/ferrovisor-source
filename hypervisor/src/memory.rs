use bootloader_api::info::{MemoryRegion, MemoryRegionKind, MemoryRegions};

pub const PAGE_SIZE: u64 = 4096;

pub struct FrameAllocator<'a> {
    regions: &'a MemoryRegions,
    cursor: u64,
}

impl<'a> FrameAllocator<'a> {
    pub fn new(regions: &'a MemoryRegions) -> Self {
        Self { regions, cursor: 0 }
    }

    pub fn allocate(&mut self) -> Result<u64, &'static str> {
        self.allocate_contiguous(1)
    }

    pub fn allocate_contiguous(&mut self, pages: u64) -> Result<u64, &'static str> {
        let bytes = pages.checked_mul(PAGE_SIZE).ok_or("allocation overflow")?;
        for region in self.regions.iter() {
            if region.kind != MemoryRegionKind::Usable {
                continue;
            }
            if let Some(start) = candidate(region, self.cursor, bytes) {
                self.cursor = start + bytes;
                return Ok(start);
            }
        }
        Err("out of physical memory")
    }
}

fn candidate(region: &MemoryRegion, cursor: u64, bytes: u64) -> Option<u64> {
    let start = align_up(region.start.max(cursor), PAGE_SIZE);
    (start.checked_add(bytes)? <= region.end).then_some(start)
}

const fn align_up(value: u64, alignment: u64) -> u64 {
    (value + alignment - 1) & !(alignment - 1)
}

pub unsafe fn zero_page(physical_offset: u64, physical: u64) {
    unsafe { core::ptr::write_bytes((physical_offset + physical) as *mut u8, 0, PAGE_SIZE as usize) };
}

pub unsafe fn page_mut(physical_offset: u64, physical: u64) -> &'static mut [u64; 512] {
    unsafe { &mut *((physical_offset + physical) as *mut [u64; 512]) }
}
