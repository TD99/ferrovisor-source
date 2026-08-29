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

    let mut frames = memory::FrameAllocator::new(&boot_info.memory_regions);
    match svm::run(&mut frames, physical_offset, vm::tiny64()) {
        Ok(()) => println!("guest stopped cleanly"),
        Err(error) => println!("hypervisor error: {}", error),
    }

    println!("system halted");
    arch::halt_forever()
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("PANIC: {}", info);
    arch::halt_forever()
}
