pub(crate) mod keyboard;
mod vmcb;
mod uefi;

use crate::{
    arch,
    console,
    external::ExternalUefiImage,
    memory::{self, FrameAllocator, PAGE_SIZE},
    vm::VmImage,
    println,
};
use core::arch::{global_asm, x86_64::__cpuid_count};
use vmcb::Vmcb;

global_asm!(include_str!("run.S"));

const SVM_CPUID_BIT: u32 = 1 << 2;
const SVM_NESTED_PAGING: u32 = 1 << 0;
const SVM_NRIP_SAVE: u32 = 1 << 3;
const EFER_SVME: u64 = 1 << 12;
const VM_CR_SVMDIS: u64 = 1 << 4;

const EXIT_CPUID: u64 = 0x72;
const EXIT_HLT: u64 = 0x78;
const EXIT_IOIO: u64 = 0x7B;
const EXIT_INVALID: u64 = u64::MAX;

const GPA_PML4: u64 = 0x1000;
const GPA_PDPT: u64 = 0x2000;
const GPA_PD: u64 = 0x3000;
const GPA_GDT: u64 = 0x5000;
const GPA_TSS: u64 = 0x6000;
const GPA_CODE: u64 = 0x10_0000;
const GPA_STACK: u64 = 0x11_0000;
const RAW_STACK_SIZE: u64 = PAGE_SIZE;
const MAX_GUEST_PAGES: usize = 4096;
const MAX_NPT_PAGE_TABLES: usize = 16;
const TWO_MIB: u64 = 2 * 1024 * 1024;

#[repr(C)]
#[derive(Default)]
struct GuestRegisters {
    rbx: u64,
    rcx: u64,
    rdx: u64,
    rsi: u64,
    rdi: u64,
    rbp: u64,
    r8: u64,
    r9: u64,
    r10: u64,
    r11: u64,
    r12: u64,
    r13: u64,
    r14: u64,
    r15: u64,
}

unsafe extern "C" {
    fn svm_vmrun(vmcb_physical: u64, registers: *mut GuestRegisters);
}

#[derive(Clone, Copy)]
struct GuestPage {
    gpa: u64,
    hpa: u64,
}

pub(super) struct GuestMemory {
    pages: [GuestPage; MAX_GUEST_PAGES],
    len: usize,
}

impl GuestMemory {
    fn new() -> Self {
        Self {
            pages: [GuestPage { gpa: 0, hpa: 0 }; MAX_GUEST_PAGES],
            len: 0,
        }
    }

    fn add(&mut self, gpa: u64, hpa: u64) -> Result<(), &'static str> {
        if self.pages[..self.len].iter().any(|page| page.gpa == gpa) {
            return Err("guest physical page was allocated twice");
        }
        if self.len == self.pages.len() {
            return Err("guest image requires too many physical pages");
        }
        self.pages[self.len] = GuestPage { gpa, hpa };
        self.len += 1;
        Ok(())
    }

    fn iter(&self) -> impl Iterator<Item = &GuestPage> {
        self.pages[..self.len].iter()
    }

    fn hpa(&self, gpa: u64) -> Result<u64, &'static str> {
        self.pages[..self.len]
            .iter()
            .find(|page| page.gpa == gpa)
            .map(|page| page.hpa)
            .ok_or("guest physical page is not mapped")
    }

    pub(super) fn copy_to(
        &self,
        physical_offset: u64,
        mut gpa: u64,
        mut source: &[u8],
    ) -> Result<(), &'static str> {
        while !source.is_empty() {
            let page_gpa = gpa & !(PAGE_SIZE - 1);
            let page_offset = (gpa - page_gpa) as usize;
            let length = source.len().min(PAGE_SIZE as usize - page_offset);
            let destination = physical_offset
                .checked_add(self.hpa(page_gpa)?)
                .and_then(|address| address.checked_add(page_offset as u64))
                .ok_or("guest physical address overflow")?;
            unsafe {
                core::ptr::copy_nonoverlapping(source.as_ptr(), destination as *mut u8, length);
            }
            source = &source[length..];
            gpa = gpa.checked_add(length as u64).ok_or("guest address overflow")?;
        }
        Ok(())
    }

    pub(super) fn copy_from(
        &self,
        physical_offset: u64,
        mut gpa: u64,
        mut destination: &mut [u8],
    ) -> Result<(), &'static str> {
        while !destination.is_empty() {
            let page_gpa = gpa & !(PAGE_SIZE - 1);
            let page_offset = (gpa - page_gpa) as usize;
            let length = destination.len().min(PAGE_SIZE as usize - page_offset);
            let source = physical_offset
                .checked_add(self.hpa(page_gpa)?)
                .and_then(|address| address.checked_add(page_offset as u64))
                .ok_or("guest physical address overflow")?;
            unsafe {
                core::ptr::copy_nonoverlapping(source as *const u8, destination.as_mut_ptr(), length);
            }
            let (_, remainder) = destination.split_at_mut(length);
            destination = remainder;
            gpa = gpa.checked_add(length as u64).ok_or("guest address overflow")?;
        }
        Ok(())
    }
}

pub fn run(
    frames: &mut FrameAllocator<'_>,
    physical_offset: u64,
    guest: VmImage,
) -> Result<(), &'static str> {
    let resources = prepare_svm(frames, physical_offset)?;
    let mut guest_memory = GuestMemory::new();
    setup_guest_core(frames, physical_offset, &mut guest_memory)?;
    allocate_guest_range(
        frames,
        physical_offset,
        &mut guest_memory,
        GPA_CODE,
        guest.image.len().max(1) as u64,
    )?;
    allocate_guest_range(
        frames,
        physical_offset,
        &mut guest_memory,
        GPA_STACK,
        RAW_STACK_SIZE,
    )?;
    load_raw_guest(&guest_memory, physical_offset, guest.image)?;
    build_guest_address_space(&guest_memory, physical_offset)?;
    let npt_root = build_nested_page_tables(frames, &guest_memory, physical_offset)?;

    let vmcb = unsafe { Vmcb::at(physical_offset + resources.vmcb_pa) };
    vmcb.initialize(
        resources.iopm_pa,
        resources.msrpm_pa,
        npt_root,
        GPA_PML4,
        GPA_TSS,
        GPA_CODE,
        GPA_STACK + RAW_STACK_SIZE - 16,
    );
    let registers = GuestRegisters::default();
    run_vcpu(
        &resources,
        &vmcb,
        guest.name,
        registers,
    )
}

pub fn run_uefi(
    frames: &mut FrameAllocator<'_>,
    physical_offset: u64,
    guest: &ExternalUefiImage,
) -> Result<(), &'static str> {
    let resources = prepare_svm(frames, physical_offset)?;
    let mut guest_memory = GuestMemory::new();
    setup_guest_core(frames, physical_offset, &mut guest_memory)?;
    let launch = uefi::prepare(frames, physical_offset, &mut guest_memory, guest.efi)?;
    build_guest_address_space(&guest_memory, physical_offset)?;
    let npt_root = build_nested_page_tables(frames, &guest_memory, physical_offset)?;

    let vmcb = unsafe { Vmcb::at(physical_offset + resources.vmcb_pa) };
    vmcb.initialize(
        resources.iopm_pa,
        resources.msrpm_pa,
        npt_root,
        GPA_PML4,
        GPA_TSS,
        launch.trampoline,
        launch.stack_pointer,
    );
    let registers = GuestRegisters::default();
    println!(
        "uefi: starting {} ({} bytes) at GPA {:#x}",
        guest.name,
        guest.efi.len(),
        launch.entry,
    );
    run_vcpu(
        &resources,
        &vmcb,
        guest.name,
        registers,
    )
}

struct SvmResources {
    vmcb_pa: u64,
    iopm_pa: u64,
    msrpm_pa: u64,
}

fn prepare_svm(
    frames: &mut FrameAllocator<'_>,
    physical_offset: u64,
) -> Result<SvmResources, &'static str> {
    arch::disable_interrupts();
    let svm_features = check_svm()?;
    println!(
        "svm: available (features={:#010x}, nested paging + NRIP present)",
        svm_features
    );

    let vmcb_pa = allocate_zeroed(frames, physical_offset)?;
    let hsave_pa = allocate_zeroed(frames, physical_offset)?;
    let iopm_pa = frames.allocate_contiguous(3)?;
    let msrpm_pa = frames.allocate_contiguous(2)?;
    unsafe {
        for page in 0..3 {
            memory::zero_page(physical_offset, iopm_pa + page * PAGE_SIZE);
        }
        core::ptr::write_bytes(
            (physical_offset + iopm_pa) as *mut u8,
            0xFF,
            3 * PAGE_SIZE as usize,
        );
        for page in 0..2 {
            memory::zero_page(physical_offset, msrpm_pa + page * PAGE_SIZE);
        }
        arch::wrmsr(arch::VM_HSAVE_PA, hsave_pa);
        arch::wrmsr(arch::EFER, arch::rdmsr(arch::EFER) | EFER_SVME);
    }
    Ok(SvmResources {
        vmcb_pa,
        iopm_pa,
        msrpm_pa,
    })
}

fn run_vcpu(
    resources: &SvmResources,
    vmcb: &Vmcb,
    guest_name: &str,
    mut registers: GuestRegisters,
) -> Result<(), &'static str> {
    println!("svm: starting {} at GPA {:#x}", guest_name, vmcb.rip());
    loop {
        unsafe { svm_vmrun(resources.vmcb_pa, &mut registers) };
        match vmcb.exit_code() {
            EXIT_IOIO => handle_io(vmcb)?,
            EXIT_CPUID => handle_cpuid(vmcb, &mut registers),
            EXIT_HLT => break,
            EXIT_INVALID => {
                println!(
                    "svm: invalid guest state (info1={:#x}, info2={:#x})",
                    vmcb.exit_info1(),
                    vmcb.exit_info2()
                );
                return Err("VMRUN rejected the VMCB guest state");
            }
            code => {
                println!(
                    "svm: unhandled VMEXIT code={:#x} info1={:#x} info2={:#x} rip={:#x}",
                    code,
                    vmcb.exit_info1(),
                    vmcb.exit_info2(),
                    vmcb.rip()
                );
                return Err("unhandled VMEXIT");
            }
        }
    }
    Ok(())
}

fn check_svm() -> Result<u32, &'static str> {
    let max_extended = __cpuid_count(0x8000_0000, 0).eax;
    if max_extended < 0x8000_000A {
        return Err("CPU does not enumerate AMD SVM");
    }
    let capabilities = __cpuid_count(0x8000_0001, 0);
    if capabilities.ecx & SVM_CPUID_BIT == 0 {
        return Err("AMD-V/SVM is not exposed; enable nested virtualization in the outer VMM");
    }
    let vm_cr = unsafe { arch::rdmsr(arch::VM_CR) };
    if vm_cr & VM_CR_SVMDIS != 0 {
        return Err("firmware or outer hypervisor locked SVM off (VM_CR.SVMDIS)");
    }
    let svm_info = __cpuid_count(0x8000_000A, 0);
    if svm_info.ebx < 2 {
        return Err("SVM reports too few guest ASIDs");
    }
    let features = svm_info.edx;
    if features & (SVM_NESTED_PAGING | SVM_NRIP_SAVE)
        != SVM_NESTED_PAGING | SVM_NRIP_SAVE
    {
        return Err("nested paging and NRIP-save are required");
    }
    Ok(features)
}

fn allocate_zeroed(
    frames: &mut FrameAllocator<'_>,
    physical_offset: u64,
) -> Result<u64, &'static str> {
    let physical = frames.allocate()?;
    unsafe { memory::zero_page(physical_offset, physical) };
    Ok(physical)
}

fn setup_guest_core(
    frames: &mut FrameAllocator<'_>,
    physical_offset: u64,
    guest_memory: &mut GuestMemory,
) -> Result<(), &'static str> {
    for gpa in [GPA_PML4, GPA_PDPT, GPA_PD, GPA_GDT, GPA_TSS] {
        guest_memory.add(gpa, allocate_zeroed(frames, physical_offset)?)?;
    }
    initialize_guest_gdt(guest_memory, physical_offset)
}

fn allocate_guest_range(
    frames: &mut FrameAllocator<'_>,
    physical_offset: u64,
    guest_memory: &mut GuestMemory,
    gpa: u64,
    length: u64,
) -> Result<(), &'static str> {
    let start = gpa & !(PAGE_SIZE - 1);
    let end = gpa
        .checked_add(length)
        .ok_or("guest allocation overflow")?;
    let end = end
        .checked_add(PAGE_SIZE - 1)
        .ok_or("guest allocation overflow")?
        & !(PAGE_SIZE - 1);
    let mut page = start;
    while page < end {
        guest_memory.add(page, allocate_zeroed(frames, physical_offset)?)?;
        page = page.checked_add(PAGE_SIZE).ok_or("guest allocation overflow")?;
    }
    Ok(())
}

fn initialize_guest_gdt(
    guest_memory: &GuestMemory,
    physical_offset: u64,
) -> Result<(), &'static str> {
    unsafe {
        let gdt = (physical_offset + guest_memory.hpa(GPA_GDT)?) as *mut u64;
        gdt.add(0).write(0);
        gdt.add(1).write(0x00AF_9A00_0000_FFFF);
        gdt.add(2).write(0x00CF_9200_0000_FFFF);
        gdt.add(3).write(tss_descriptor_low(GPA_TSS));
        gdt.add(4).write(GPA_TSS >> 32);
        ((physical_offset + guest_memory.hpa(GPA_TSS)?) as *mut u8)
            .add(102)
            .cast::<u16>()
            .write(104);
    }
    Ok(())
}

fn load_raw_guest(
    guest_memory: &GuestMemory,
    physical_offset: u64,
    image: &[u8],
) -> Result<(), &'static str> {
    if image.len() > PAGE_SIZE as usize {
        return Err("guest image exceeds one page");
    }
    guest_memory.copy_to(physical_offset, GPA_CODE, image)
}

fn build_guest_address_space(
    guest_memory: &GuestMemory,
    physical_offset: u64,
) -> Result<(), &'static str> {
    let max_gpa = guest_memory
        .iter()
        .map(|page| page.gpa.checked_add(PAGE_SIZE))
        .try_fold(0, |max, end| end.map(|end| max.max(end)))
        .ok_or("guest address overflow")?;
    let pd_entries = max_gpa
        .checked_add(TWO_MIB - 1)
        .ok_or("guest address space overflow")?
        / TWO_MIB;
    if pd_entries == 0 || pd_entries > 512 {
        return Err("guest address space exceeds 1 GiB");
    }
    unsafe {
        let pml4 = memory::page_mut(physical_offset, guest_memory.hpa(GPA_PML4)?);
        let pdpt = memory::page_mut(physical_offset, guest_memory.hpa(GPA_PDPT)?);
        let pd = memory::page_mut(physical_offset, guest_memory.hpa(GPA_PD)?);
        pml4[0] = GPA_PDPT | 0x003;
        pdpt[0] = GPA_PD | 0x003;
        for index in 0..pd_entries as usize {
            pd[index] = (index as u64 * TWO_MIB) | 0x083;
        }
    }
    Ok(())
}

const fn tss_descriptor_low(base: u64) -> u64 {
    0x67
        | ((base & 0x00FF_FFFF) << 16)
        | (0x89 << 40)
        | ((base & 0xFF00_0000) << 32)
}

fn build_nested_page_tables(
    frames: &mut FrameAllocator<'_>,
    guest_memory: &GuestMemory,
    offset: u64,
) -> Result<u64, &'static str> {
    let pml4_pa = allocate_zeroed(frames, offset)?;
    let pdpt_pa = allocate_zeroed(frames, offset)?;
    let pd_pa = allocate_zeroed(frames, offset)?;
    let mut page_tables = [
        GuestPage { gpa: 0, hpa: 0 };
        MAX_NPT_PAGE_TABLES
    ];
    let mut page_table_count = 0;
    unsafe {
        let pml4 = memory::page_mut(offset, pml4_pa);
        let pdpt = memory::page_mut(offset, pdpt_pa);
        let pd = memory::page_mut(offset, pd_pa);
        pml4[0] = pdpt_pa | 0x007;
        pdpt[0] = pd_pa | 0x007;
        for page in guest_memory.iter() {
            let region = page.gpa & !(TWO_MIB - 1);
            let table = if let Some(table) = page_tables[..page_table_count]
                .iter()
                .find(|table| table.gpa == region)
            {
                table.hpa
            } else {
                if page_table_count == page_tables.len() {
                    return Err("guest address space needs too many page tables");
                }
                let table = page_tables.get_mut(page_table_count).ok_or("page table overflow")?;
                table.gpa = region;
                table.hpa = allocate_zeroed(frames, offset)?;
                page_table_count += 1;
                let pd_index = ((region >> 21) & 0x1FF) as usize;
                pd[pd_index] = table.hpa | 0x007;
                table.hpa
            };
            let pt = memory::page_mut(offset, table);
            let index = ((page.gpa >> 12) & 0x1FF) as usize;
            pt[index] = page.hpa | 0x007;
        }
    }
    Ok(pml4_pa)
}

fn handle_io(vmcb: &Vmcb) -> Result<(), &'static str> {
    let info = vmcb.exit_info1();
    let is_input = info & 1 != 0;
    let is_string = info & (1 << 2) != 0;
    let is_rep = info & (1 << 3) != 0;
    let port = (info >> 16) as u16;
    if is_string || is_rep {
        return Err("string/REP port I/O is not implemented");
    }
    if is_input {
        if port != 0x3F8 {
            return Err("guest read an unsupported I/O port");
        }
        let byte = console::read_byte_blocking();
        vmcb.set_rax((vmcb.rax() & !0xFF) | byte as u64);
    } else {
        match port {
            0x3F8 => console::write_byte(vmcb.rax() as u8),
            0x3F9 => console::set_cursor_column(vmcb.rax() as usize),
            0x3FA => console::set_cursor_row(vmcb.rax() as usize),
            0x3FB => console::set_attribute(vmcb.rax() as u8),
            _ => return Err("guest accessed an unsupported I/O port"),
        }
    }
    vmcb.advance_to_nrip();
    Ok(())
}

fn handle_cpuid(vmcb: &Vmcb, registers: &mut GuestRegisters) {
    let result = __cpuid_count(vmcb.rax() as u32, registers.rcx as u32);
    vmcb.set_rax(result.eax as u64);
    registers.rbx = result.ebx as u64;
    registers.rcx = result.ecx as u64;
    registers.rdx = result.edx as u64;
    vmcb.advance_to_nrip();
}
