mod guest;
mod keyboard;
mod vmcb;

use crate::{
    arch,
    console,
    memory::{self, FrameAllocator, PAGE_SIZE},
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
const GPA_PT: u64 = 0x4000;
const GPA_GDT: u64 = 0x5000;
const GPA_TSS: u64 = 0x6000;
const GPA_CODE: u64 = 0x10_0000;
const GPA_STACK: u64 = 0x11_0000;

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

struct GuestPage {
    gpa: u64,
    hpa: u64,
}

pub fn run(frames: &mut FrameAllocator<'_>, physical_offset: u64) -> Result<(), &'static str> {
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
        core::ptr::write_bytes((physical_offset + iopm_pa) as *mut u8, 0xFF, 3 * PAGE_SIZE as usize);
        for page in 0..2 {
            memory::zero_page(physical_offset, msrpm_pa + page * PAGE_SIZE);
        }
    }

    let mut pages = [
        GuestPage { gpa: GPA_PML4, hpa: 0 },
        GuestPage { gpa: GPA_PDPT, hpa: 0 },
        GuestPage { gpa: GPA_PD, hpa: 0 },
        GuestPage { gpa: GPA_PT, hpa: 0 },
        GuestPage { gpa: GPA_GDT, hpa: 0 },
        GuestPage { gpa: GPA_TSS, hpa: 0 },
        GuestPage { gpa: GPA_CODE, hpa: 0 },
        GuestPage { gpa: GPA_STACK, hpa: 0 },
    ];
    for page in &mut pages {
        page.hpa = allocate_zeroed(frames, physical_offset)?;
    }

    build_guest_address_space(&pages, physical_offset)?;
    load_guest(&pages, physical_offset)?;
    let npt_root = build_nested_page_tables(frames, &pages, physical_offset)?;

    let vmcb = unsafe { Vmcb::at(physical_offset + vmcb_pa) };
    vmcb.initialize(
        iopm_pa,
        msrpm_pa,
        npt_root,
        GPA_PML4,
        GPA_TSS,
        GPA_CODE,
        GPA_STACK + PAGE_SIZE - 16,
    );

    unsafe {
        arch::wrmsr(arch::VM_HSAVE_PA, hsave_pa);
        arch::wrmsr(arch::EFER, arch::rdmsr(arch::EFER) | EFER_SVME);
    }

    println!("svm: entering guest at GPA {:#x}", GPA_CODE);
    let mut registers = GuestRegisters::default();
    loop {
        unsafe { svm_vmrun(vmcb_pa, &mut registers) };
        match vmcb.exit_code() {
            EXIT_IOIO => handle_io(&vmcb)?,
            EXIT_CPUID => handle_cpuid(&vmcb, &mut registers),
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

fn hpa(pages: &[GuestPage], gpa: u64) -> Result<u64, &'static str> {
    pages
        .iter()
        .find(|page| page.gpa == gpa)
        .map(|page| page.hpa)
        .ok_or("missing guest page")
}

fn build_guest_address_space(
    pages: &[GuestPage],
    offset: u64,
) -> Result<(), &'static str> {
    unsafe {
        let pml4 = memory::page_mut(offset, hpa(pages, GPA_PML4)?);
        let pdpt = memory::page_mut(offset, hpa(pages, GPA_PDPT)?);
        let pd = memory::page_mut(offset, hpa(pages, GPA_PD)?);
        let pt = memory::page_mut(offset, hpa(pages, GPA_PT)?);
        pml4[0] = GPA_PDPT | 0x003;
        pdpt[0] = GPA_PD | 0x003;
        pd[0] = GPA_PT | 0x003;
        for page in pages {
            let index = ((page.gpa >> 12) & 0x1FF) as usize;
            pt[index] = page.gpa | 0x003;
        }
    }
    Ok(())
}

fn load_guest(pages: &[GuestPage], offset: u64) -> Result<(), &'static str> {
    let image = guest::image();
    if image.len() > PAGE_SIZE as usize {
        return Err("embedded guest exceeds one page");
    }
    unsafe {
        core::ptr::copy_nonoverlapping(
            image.as_ptr(),
            (offset + hpa(pages, GPA_CODE)?) as *mut u8,
            image.len(),
        );
        let gdt = (offset + hpa(pages, GPA_GDT)?) as *mut u64;
        gdt.add(0).write(0);
        gdt.add(1).write(0x00AF_9A00_0000_FFFF);
        gdt.add(2).write(0x00CF_9200_0000_FFFF);
        gdt.add(3).write(tss_descriptor_low(GPA_TSS));
        gdt.add(4).write(GPA_TSS >> 32);
        ((offset + hpa(pages, GPA_TSS)?) as *mut u8)
            .add(102)
            .cast::<u16>()
            .write(104);
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
    pages: &[GuestPage],
    offset: u64,
) -> Result<u64, &'static str> {
    let pml4_pa = allocate_zeroed(frames, offset)?;
    let pdpt_pa = allocate_zeroed(frames, offset)?;
    let pd_pa = allocate_zeroed(frames, offset)?;
    let pt_pa = allocate_zeroed(frames, offset)?;
    unsafe {
        let pml4 = memory::page_mut(offset, pml4_pa);
        let pdpt = memory::page_mut(offset, pdpt_pa);
        let pd = memory::page_mut(offset, pd_pa);
        let pt = memory::page_mut(offset, pt_pa);
        pml4[0] = pdpt_pa | 0x007;
        pdpt[0] = pd_pa | 0x007;
        pd[0] = pt_pa | 0x007;
        for page in pages {
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
    if port != 0x3F8 {
        return Err("guest accessed an unsupported I/O port");
    }
    if is_input {
        let byte = keyboard::read_ascii_blocking();
        vmcb.set_rax((vmcb.rax() & !0xFF) | byte as u64);
    } else {
        console::write_guest_byte(vmcb.rax() as u8);
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
