use std::{env, path::PathBuf};

fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR missing"));
    let kernel = PathBuf::from(
        env::var_os("CARGO_BIN_FILE_HYPERVISOR_hypervisor")
            .expect("hypervisor artifact path missing"),
    );

    let bios = out.join("ferrovisor-bios.img");
    bootloader::BiosBoot::new(&kernel)
        .create_disk_image(&bios)
        .expect("failed to create BIOS image");

    let uefi = out.join("ferrovisor-uefi.img");
    bootloader::UefiBoot::new(&kernel)
        .create_disk_image(&uefi)
        .expect("failed to create UEFI image");

    println!("cargo:rustc-env=FERROVISOR_BIOS={}", bios.display());
    println!("cargo:rustc-env=FERROVISOR_UEFI={}", uefi.display());
}

