use core::ptr::{read_volatile, write_volatile};

const INTERCEPT_MISC1: usize = 0x00C;
const IOPM_BASE_PA: usize = 0x040;
const MSRPM_BASE_PA: usize = 0x048;
const GUEST_ASID: usize = 0x058;
const TLB_CONTROL: usize = 0x05C;
const EXIT_CODE: usize = 0x070;
const EXIT_INFO1: usize = 0x078;
const EXIT_INFO2: usize = 0x080;
const NP_ENABLE: usize = 0x090;
const N_CR3: usize = 0x0B0;
const NRIP: usize = 0x0C8;

const SAVE: usize = 0x400;
const ES: usize = SAVE + 0x00;
const CS: usize = SAVE + 0x10;
const SS: usize = SAVE + 0x20;
const DS: usize = SAVE + 0x30;
const FS: usize = SAVE + 0x40;
const GS: usize = SAVE + 0x50;
const GDTR: usize = SAVE + 0x60;
const IDTR: usize = SAVE + 0x80;
const TR: usize = SAVE + 0x90;
const CPL: usize = SAVE + 0xCB;
const EFER: usize = SAVE + 0xD0;
const CR4: usize = SAVE + 0x148;
const CR3: usize = SAVE + 0x150;
const CR0: usize = SAVE + 0x158;
const DR7: usize = SAVE + 0x160;
const DR6: usize = SAVE + 0x168;
const RFLAGS: usize = SAVE + 0x170;
const RIP: usize = SAVE + 0x178;
const RSP: usize = SAVE + 0x1D8;
const RAX: usize = SAVE + 0x1F8;
const G_PAT: usize = SAVE + 0x268;

const INTERCEPT_CPUID: u32 = 1 << 18;
const INTERCEPT_HLT: u32 = 1 << 24;
const INTERCEPT_IOIO: u32 = 1 << 27;

pub struct Vmcb {
    base: *mut u8,
}

impl Vmcb {
    pub unsafe fn at(virtual_address: u64) -> Self {
        Self { base: virtual_address as *mut u8 }
    }

    pub fn initialize(
        &self,
        iopm_pa: u64,
        msrpm_pa: u64,
        npt_root: u64,
        guest_cr3: u64,
        tss_base: u64,
        rip: u64,
        rsp: u64,
    ) {
        self.write_u32(
            INTERCEPT_MISC1,
            INTERCEPT_CPUID | INTERCEPT_HLT | INTERCEPT_IOIO,
        );
        self.write_u64(IOPM_BASE_PA, iopm_pa);
        self.write_u64(MSRPM_BASE_PA, msrpm_pa);
        self.write_u32(GUEST_ASID, 1);
        self.write_u8(TLB_CONTROL, 1);
        self.write_u64(NP_ENABLE, 1);
        self.write_u64(N_CR3, npt_root);

        self.segment(CS, 0x08, 0x0A9B, 0xFFFF_FFFF, 0);
        self.segment(SS, 0x10, 0x0C93, 0xFFFF_FFFF, 0);
        self.segment(DS, 0x10, 0x0C93, 0xFFFF_FFFF, 0);
        self.segment(ES, 0x10, 0x0C93, 0xFFFF_FFFF, 0);
        self.segment(FS, 0x10, 0x0C93, 0xFFFF_FFFF, 0);
        self.segment(GS, 0x10, 0x0C93, 0xFFFF_FFFF, 0);
        self.segment(TR, 0x18, 0x008B, 0x67, tss_base);
        self.segment(GDTR, 0, 0, 0x27, 0x5000);
        self.segment(IDTR, 0, 0, 0, 0);
        self.write_u8(CPL, 0);
        self.write_u64(EFER, (1 << 8) | (1 << 10));
        self.write_u64(CR4, 1 << 5);
        self.write_u64(CR3, guest_cr3);
        self.write_u64(CR0, 0x8001_0033);
        self.write_u64(DR6, 0xFFFF_0FF0);
        self.write_u64(DR7, 0x400);
        self.write_u64(RFLAGS, 2);
        self.write_u64(RIP, rip);
        self.write_u64(RSP, rsp);
        self.write_u64(RAX, 0);
        self.write_u64(G_PAT, 0x0007_0406_0007_0406);
    }

    fn segment(&self, offset: usize, selector: u16, attributes: u16, limit: u32, base: u64) {
        self.write_u16(offset, selector);
        self.write_u16(offset + 2, attributes);
        self.write_u32(offset + 4, limit);
        self.write_u64(offset + 8, base);
    }

    pub fn exit_code(&self) -> u64 { self.read_u64(EXIT_CODE) }
    pub fn exit_info1(&self) -> u64 { self.read_u64(EXIT_INFO1) }
    pub fn exit_info2(&self) -> u64 { self.read_u64(EXIT_INFO2) }
    pub fn rip(&self) -> u64 { self.read_u64(RIP) }
    pub fn rax(&self) -> u64 { self.read_u64(RAX) }
    pub fn set_rax(&self, value: u64) { self.write_u64(RAX, value) }
    pub fn advance_to_nrip(&self) { self.write_u64(RIP, self.read_u64(NRIP)) }

    fn read_u64(&self, offset: usize) -> u64 {
        unsafe { read_volatile(self.base.add(offset).cast()) }
    }
    fn write_u64(&self, offset: usize, value: u64) {
        unsafe { write_volatile(self.base.add(offset).cast(), value) }
    }
    fn write_u32(&self, offset: usize, value: u32) {
        unsafe { write_volatile(self.base.add(offset).cast(), value) }
    }
    fn write_u16(&self, offset: usize, value: u16) {
        unsafe { write_volatile(self.base.add(offset).cast(), value) }
    }
    fn write_u8(&self, offset: usize, value: u8) {
        unsafe { write_volatile(self.base.add(offset).cast(), value) }
    }
}
