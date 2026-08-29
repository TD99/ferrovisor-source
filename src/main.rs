use std::{
    env,
    fs, io,
    path::Path,
    process::{Command, ExitStatus},
};

#[derive(Clone, Copy, PartialEq)]
enum Target {
    Baremetal,
    Vmware,
    Hyperv,
}

impl Target {
    fn parse() -> io::Result<Self> {
        let mut args = env::args_os().skip(1);
        let Some(argument) = args.next() else {
            return Ok(Self::Baremetal);
        };
        if argument != "--target" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "usage: ferrovisor-image [--target baremetal|vmware|hyperv]",
            ));
        }
        let Some(target) = args.next() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "--target requires baremetal, vmware, or hyperv",
            ));
        };
        if args.next().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "usage: ferrovisor-image [--target baremetal|vmware|hyperv]",
            ));
        }
        match target.to_str() {
            Some("baremetal") => Ok(Self::Baremetal),
            Some("vmware") => Ok(Self::Vmware),
            Some("hyperv") => Ok(Self::Hyperv),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "--target must be baremetal, vmware, or hyperv",
            )),
        }
    }
}

fn copy(source: &str, destination: &Path) -> io::Result<()> {
    fs::copy(source, destination)?;
    println!("created {}", destination.display());
    Ok(())
}

fn write_vmdk(image: &Path, descriptor: &Path) -> io::Result<()> {
    let bytes = fs::metadata(image)?.len();
    if bytes % 512 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "raw disk image size is not sector aligned",
        ));
    }
    let sectors = bytes / 512;
    let cylinders = sectors.div_ceil(16 * 63).max(1);
    let image_name = image
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "non-UTF-8 image name"))?;
    let contents = format!(
        "# Disk DescriptorFile\n\
         version=1\n\
         encoding=\"UTF-8\"\n\
         CID=fffffffe\n\
         parentCID=ffffffff\n\
         createType=\"monolithicFlat\"\n\n\
         RW {sectors} FLAT \"{image_name}\" 0\n\n\
         ddb.adapterType = \"ide\"\n\
         ddb.geometry.cylinders = \"{cylinders}\"\n\
         ddb.geometry.heads = \"16\"\n\
         ddb.geometry.sectors = \"63\"\n\
         ddb.virtualHWVersion = \"20\"\n"
    );
    fs::write(descriptor, contents)?;
    println!("created {}", descriptor.display());
    Ok(())
}

fn write_vmx(vmdk: &Path, config: &Path, firmware: &str) -> io::Result<()> {
    let disk_name = vmdk
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "non-UTF-8 VMDK name"))?;
    let display_name = if firmware == "bios" {
        "Ferrovisor BIOS"
    } else {
        "Ferrovisor UEFI"
    };
    let contents = format!(
        ".encoding = \"windows-1252\"\n\
         config.version = \"8\"\n\
         virtualHW.version = \"20\"\n\
         displayName = \"{display_name}\"\n\
         guestOS = \"other-64\"\n\
         firmware = \"{firmware}\"\n\
         uefi.secureBoot.enabled = \"FALSE\"\n\
         memsize = \"512\"\n\
         numvcpus = \"1\"\n\
         cpuid.coresPerSocket = \"1\"\n\
         vhv.enable = \"TRUE\"\n\
         ide0.present = \"TRUE\"\n\
         ide0:0.present = \"TRUE\"\n\
         ide0:0.deviceType = \"disk\"\n\
         ide0:0.fileName = \"{disk_name}\"\n\
         floppy0.present = \"FALSE\"\n"
    );
    fs::write(config, contents)?;
    println!("created {}", config.display());
    Ok(())
}

fn run_qemu_img(arguments: &[&str]) -> io::Result<ExitStatus> {
    Command::new("qemu-img")
        .args(arguments)
        .status()
        .map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "qemu-img is required for the hyperv target; install QEMU and add it to PATH",
                )
            } else {
                error
            }
        })
}

fn write_vhdx(image: &Path, disk: &Path) -> io::Result<()> {
    let status = run_qemu_img(&[
        "convert",
        "-S",
        "0",
        "-f",
        "raw",
        "-O",
        "vhdx",
        "-o",
        "subformat=fixed",
        image.to_str().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "non-UTF-8 image path")
        })?,
        disk.to_str().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "non-UTF-8 VHDX path")
        })?,
    ])?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "qemu-img failed while creating {}; stop any Hyper-V VM using this disk and detach it before rebuilding",
            disk.display(),
        )));
    }
    clear_sparse_attribute(disk)?;
    println!("created {}", disk.display());
    Ok(())
}

#[cfg(windows)]
fn clear_sparse_attribute(disk: &Path) -> io::Result<()> {
    let status = Command::new("fsutil")
        .args(["sparse", "setflag"])
        .arg(disk)
        .arg("0")
        .status()?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "failed to clear the sparse attribute on {}",
            disk.display()
        )));
    }
    Ok(())
}

#[cfg(not(windows))]
fn clear_sparse_attribute(_: &Path) -> io::Result<()> {
    Ok(())
}

fn main() -> io::Result<()> {
    let target = Target::parse()?;
    let dist = Path::new("dist");
    fs::create_dir_all(dist)?;
    let bios = dist.join("ferrovisor-bios.img");
    let uefi = dist.join("ferrovisor-uefi.img");
    copy(env!("FERROVISOR_BIOS"), &bios)?;
    copy(env!("FERROVISOR_UEFI"), &uefi)?;
    match target {
        Target::Baremetal => {}
        Target::Vmware => {
            write_vmdk(&bios, &dist.join("ferrovisor-bios.vmdk"))?;
            write_vmdk(&uefi, &dist.join("ferrovisor-uefi.vmdk"))?;
            write_vmx(
                &dist.join("ferrovisor-bios.vmdk"),
                &dist.join("ferrovisor-bios.vmx"),
                "bios",
            )?;
            write_vmx(
                &dist.join("ferrovisor-uefi.vmdk"),
                &dist.join("ferrovisor-uefi.vmx"),
                "efi",
            )?;
        }
        Target::Hyperv => {
            write_vhdx(&bios, &dist.join("ferrovisor-bios.vhdx"))?;
            write_vhdx(&uefi, &dist.join("ferrovisor-uefi.vhdx"))?;
        }
    }
    Ok(())
}
