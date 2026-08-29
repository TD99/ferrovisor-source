#![no_std]
#![no_main]

mod arch;
mod console;
mod external;
mod memory;
mod svm;
mod vm;

use bootloader_api::{BootInfo, BootloaderConfig, config::Mapping};
use core::panic::PanicInfo;

const BOOT_CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();
    config.mappings.physical_memory = Some(Mapping::Dynamic);
    config.kernel_stack_size = 256 * 1024;
    config
};

bootloader_api::entry_point!(kernel_main, config = &BOOT_CONFIG);

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    let physical_offset = boot_info
        .physical_memory_offset
        .into_option()
        .expect("physical memory mapping was not supplied");

    console::init(physical_offset);
    console::show_startup_screen();
    println!("Ferrovisor 0.1 - x86_64 AMD-V type-1 hypervisor");
    println!("boot: Rust kernel entered at CPL0");
    println!("console: use .\\scripts\\hyperv-console.ps1 -PipeName ferrovisor-bios-com1 for Hyper-V serial I/O");

    let vms = vm::available();
    loop {
        let Some(selection) = select_vm(&vms) else {
            println!("hypervisor: halted by operator");
            arch::halt_forever();
        };
        let mut frames = memory::FrameAllocator::new(&boot_info.memory_regions);
        match selection {
            MenuSelection::Vm(guest) => match svm::run(&mut frames, physical_offset, guest) {
                Ok(()) => println!("vm: {} stopped; returning to manager", guest.name),
                Err(error) => println!("hypervisor error: {}", error),
            },
            MenuSelection::ExternalUefi(guest) => {
                match svm::run_uefi(&mut frames, physical_offset, guest) {
                    Ok(()) => println!("vm: {} stopped; returning to manager", guest.name),
                    Err(error) => println!("hypervisor error: {}", error),
                }
            }
        }
    }
}

enum MenuSelection {
    Vm(vm::VmImage),
    ExternalUefi(&'static external::ExternalUefiImage),
}

fn select_vm(vms: &[vm::VmImage]) -> Option<MenuSelection> {
    println!("");
    println!("Virtual Machine Manager");
    for (index, guest) in vms.iter().enumerate() {
        println!("  [{}] {} - {}", index + 1, guest.name, guest.description);
    }
    let external_start = vms.len() + 1;
    for (index, guest) in vm::external_uefi().iter().enumerate() {
        println!("  [{}] {} - imported UEFI disk", external_start + index, guest.name);
    }
    println!("  [0] Halt Ferrovisor");
    println!("Select a VM by number, or press Enter for {}.", vms[0].name);

    loop {
        let key = console::read_byte_blocking();
        if key == b'\n' || key == b'\r' {
            println!("vm: selected {}", vms[0].name);
            return Some(MenuSelection::Vm(vms[0]));
        }
        if key == b'0' {
            return None;
        }
        if key >= b'1' {
            let index = (key - b'1') as usize;
            if let Some(guest) = vms.get(index) {
                console::write_byte(key);
                println!("");
                println!("vm: selected {}", guest.name);
                return Some(MenuSelection::Vm(*guest));
            }
            let external_index = index.saturating_sub(vms.len());
            if let Some(guest) = vm::external_uefi().get(external_index) {
                console::write_byte(key);
                println!("");
                println!("vm: selected {}", guest.name);
                return Some(MenuSelection::ExternalUefi(guest));
            }
        }
        println!("Invalid selection. Choose a listed VM.");
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("PANIC: {}", info);
    arch::halt_forever()
}
