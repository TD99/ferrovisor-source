#![no_std]
#![no_main]

mod arch;
mod console;
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
        let Some(selected_vm) = select_vm(&vms) else {
            println!("hypervisor: halted by operator");
            arch::halt_forever();
        };
        let mut frames = memory::FrameAllocator::new(&boot_info.memory_regions);
        match svm::run(&mut frames, physical_offset, selected_vm) {
            Ok(()) => println!("vm: {} stopped; returning to manager", selected_vm.name),
            Err(error) => println!("hypervisor error: {}", error),
        }
    }
}

fn select_vm(vms: &[vm::VmImage]) -> Option<vm::VmImage> {
    println!("");
    println!("Virtual Machine Manager");
    for (index, guest) in vms.iter().enumerate() {
        println!("  [{}] {} - {}", index + 1, guest.name, guest.description);
    }
    println!("  [0] Halt Ferrovisor");
    println!("Select a VM by number, or press Enter for {}.", vms[0].name);

    loop {
        let key = console::read_byte_blocking();
        if key == b'\n' || key == b'\r' {
            println!("vm: selected {}", vms[0].name);
            return Some(vms[0]);
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
                return Some(*guest);
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
