# Ferrovisor

Ferrovisor is a small educational type-1 (bare-metal) hypervisor for x86_64 AMD
processors. It boots directly through BIOS or UEFI, enables AMD-V/SVM, creates a
VMCB and nested page tables, and presents a catalog of 64-bit guest images.

This milestone intentionally virtualizes the included Tiny64 guest rather than
Linux. Linux support needs an ELF/bzImage loader, a larger guest-physical memory
manager, interrupt virtualization, timers, and substantially more device
emulation; those are kept out of the first trustworthy bootable core.

## What works

- BIOS and UEFI bootable raw disk images (BIOS is the recommended VMware path)
- AMD SVM capability and lock checks
- AMD nested paging (guest virtual -> guest physical -> host physical)
- Long-mode guest execution at CPL0
- Correct guest GPR preservation around `VMRUN`
- `CPUID`, `IOIO`, and `HLT` VMEXIT handling
- Virtual COM1 connected to the VGA text console and host serial port
- Serial-pipe and PS/2 keyboard input for the VM selector and guest shell
- A pre-boot VM selector with registered guest images
- Tiny64 commands: `help`, `info`, `ticks`, `echo`, `ls`, `pwd`, `cat`, `clear`, and `halt`

This is an educational prototype, not a security boundary or production VMM.
It is single-vCPU, AMD-only, uses a US set-1 keyboard map, and deliberately
stops on any unexpected VM exit.

### Validation status

The assembly payloads have been assembled and inspected, have no remaining
relocations, and the Tiny64 image fits within the current 4 KiB raw-image limit. The source was produced in an
environment without a Rust toolchain or an AMD nested-virtualization target, so
the final Cargo build and VMware/AMD-V boot must be performed on the target
Windows machine. Treat the first run as hardware bring-up, not as a previously
certified release.

## Requirements

- An x86_64 Windows 11 machine with an AMD CPU that supports AMD-V/RVI
- VMware Workstation 17.x or newer
- Rust installed through `rustup`
- Visual Studio C++ Build Tools on Windows, or a C compiler and `ar` on Unix
- PowerShell 5+ (or a Unix-like shell for `scripts/build.sh`)
- QEMU (`qemu-img`) only when building the Hyper-V target

The checked-in toolchain file installs nightly Rust, the bare-metal target,
`rust-src`, and LLVM tools automatically on the first build. The build scripts
also compile a small UEFI C-runtime compatibility shim required by this nightly.

## Build on Windows 11

From PowerShell in the repository root:

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\scripts\build.ps1
```

The default target is `baremetal`, which produces raw BIOS and UEFI images:

```text
dist/ferrovisor-bios.img
dist/ferrovisor-uefi.img
```

Choose a platform target when additional disk formats are needed:

```powershell
# Generate VMware VMDK and VMX files.
.\scripts\build.ps1 --target vmware

# Generate Hyper-V VHDX disks. Requires qemu-img on PATH.
.\scripts\build.ps1 --target hyperv
```

`vmware` adds `.vmdk` descriptors and ready-to-open `.vmx` files. Keep each
VMware image, descriptor, and VMX file together. `hyperv` adds
`ferrovisor-bios.vhdx` and `ferrovisor-uefi.vhdx`.
Before rebuilding the Hyper-V target, power off and detach any VM using either
VHDX file so QEMU can replace it.

## VMware Workstation setup

### Generated configuration

1. In VMware Workstation, choose **File > Open** and select
   `dist/ferrovisor-bios.vmx` (recommended) or `dist/ferrovisor-uefi.vmx`.
2. Select **I Copied It** if VMware asks how the VM was obtained.
3. Boot the VM, select Tiny64 from the Ferrovisor VM menu, then use the `tiny>`
   prompt.

The generated VMX files configure **Other / Other 64-bit**, one vCPU/core,
512 MB RAM, an IDE disk, and nested virtualization (`vhv.enable = "TRUE"`).
The UEFI VMX also disables Secure Boot because the bootloader is unsigned.
Do not move a `.vmx` file away from its matching `.vmdk` and `.img` files.

### Manual configuration

1. Create a new custom VM and choose **I will install the operating system later**.
2. Select **Other / Other 64-bit**, one processor, one core, and 512 MB RAM.
3. In **VM Settings > Processors**, enable **Virtualize Intel VT-x/EPT or
   AMD-V/RVI**. Despite the mixed label, this is the nested-virtualization
   switch required for AMD SVM to be visible inside Ferrovisor.
4. Remove the automatically created disk. Add **Hard Disk > IDE > Use an
   existing virtual disk**, choose `dist/ferrovisor-bios.vmdk`, and keep its
   existing format if VMware asks to convert it.
5. Use legacy BIOS firmware for the BIOS image. For the UEFI image, change the
   VM firmware to UEFI, disable Secure Boot, and attach
   `dist/ferrovisor-uefi.vmdk` instead.
6. Boot, select Tiny64 from the Ferrovisor VM menu, then use the `tiny>` prompt.

The VGA text console is designed around the recommended BIOS configuration.
The UEFI image is generated for bring-up and serial-console work; graphical GOP
rendering is not implemented yet, so its output might require a VMware virtual
serial port.

The optional [`vmware/ferrovisor.vmx.fragment`](vmware/ferrovisor.vmx.fragment)
records the important VMX settings. Prefer VMware's UI for attaching the disk.

## Hyper-V setup

1. Build with `.\scripts\build.ps1 --target hyperv`.
2. Create a **Generation 1** VM for `dist/ferrovisor-bios.vhdx`, or a
   **Generation 2** VM for `dist/ferrovisor-uefi.vhdx` with Secure Boot disabled.
3. Set the VM to 512 MB RAM and one virtual processor.
4. Enable nested virtualization for the VM you are booting. For the recommended
   BIOS VM, run: `Set-VMProcessor -VMName ferrovisor-bios -ExposeVirtualizationExtensions $true`.
5. Configure COM1 as a normal named-pipe serial port:

```powershell
Set-VMComPort -VMName Ferrovisor -Number 1 -Path "\\.\pipe\ferrovisor-com1" -DebuggerMode Off
```

For a Generation 2 VM, explicitly make the attached VHDX the first UEFI boot
device:

```powershell
$disk = Get-VMHardDiskDrive -VMName Ferrovisor | Where-Object Path -Match "ferrovisor-uefi\.vhdx$"
Set-VMFirmware -VMName Ferrovisor -FirstBootDevice $disk
```

Open a second PowerShell window and run the included pipe reader before starting
the VM:

```powershell
    .\scripts\hyperv-console.ps1 -PipeName ferrovisor-bios-com1
```

The reader waits for Hyper-V to create the pipe, accepts typed guest input, and prints COM1 output,
including the boot banner and SVM capability errors. If it connects after the
VM boots, restart the VM to replay its startup output. PuTTY's serial backend
is not recommended for this named-pipe connection. It runs until disconnected by
default; use `-TimeoutSeconds 60` to set a timeout.

For a Generation 2 VM, disable Secure Boot with:

```powershell
Set-VMFirmware -VMName Ferrovisor -EnableSecureBoot Off
```

The Hyper-V error `The unsigned image's hash is not allowed DB` means Secure
Boot is still enabled. It is expected until this setting is applied.

Hyper-V must expose AMD SVM to the guest. If Ferrovisor reports that SVM is not
exposed, the host/Hyper-V version does not provide the required nested AMD-V
capability.

The generated VHDX disks are fixed and have their Windows sparse attribute
cleared. This is required for Hyper-V checkpoints.

Hyper-V VMConnect can remain black after a successful boot. The BIOS image uses
legacy VGA text mode, while the UEFI image deliberately has no GOP framebuffer
renderer; use COM1 to observe either target. A QEMU serial boot of the BIOS
image reaches the Ferrovisor banner and SVM capability check.

### Windows VBS / Hyper-V caveat

VMware Workstation can run through Windows Hypervisor Platform when Hyper-V or
VBS is active, but that Host VBS mode does **not** expose x86 virtualization
extensions to nested guests. If Ferrovisor prints that SVM is not exposed,
disable Memory Integrity/VBS and the Hyper-V hypervisor, reboot, then confirm
the VMware processor virtualization checkbox is enabled. This affects host
security; understand your environment's policy before changing it.

On VMware Workstation 25, Host VBS can instead fail before the VM reaches
firmware with the generic **Failed to start the virtual machine** dialog. The
corresponding `dist/vmware.log` lines are `Hyper-V detected by CPUID` followed
by `ULM: Failed to set up partition, res 0xc0350005`. This is a host
virtualization conflict, not an image or VMX parsing failure; disable
Memory Integrity/VBS and the Hyper-V hypervisor, then reboot before starting
either VM configuration.

## Architecture

```text
ferrovisor-image/       host-side image builder
hypervisor/src/
  main.rs               boot entry and lifecycle
  arch.rs               x86 port/MSR/interrupt primitives
  console.rs            VGA + physical COM1 console
  memory.rs             boot-memory frame allocator
  svm/
    mod.rs              SVM lifecycle, NPT, VMEXIT dispatch
    vmcb.rs             typed VMCB offset access
    run.S               VMRUN register-preservation trampoline
    keyboard.rs         PS/2 set-1 input
  vm.rs                 VM image catalog and guest-image boundary
guests/
  tiny64/tiny64.S       position-independent Tiny64 guest image
scripts/                Windows and Unix build entrypoints
vmware/                 VMware configuration notes
```

The selected guest uses four-level identity paging. A second four-level page-table tree
implements AMD nested paging and maps only eight guest pages. All guest COM1
accesses are intercepted. Output is mirrored to VGA and the real COM1; input
blocks in the hypervisor until serial-pipe or supported PS/2 input arrives.

## Guests and VM Catalog

The hypervisor owns the VM catalog in `hypervisor/src/vm.rs`; guest payloads live
outside the hypervisor in `guests/`. Each catalog entry is a `VmImage`, so adding
another guest does not require placing its source or symbols under `svm/`.

The built-in Tiny64 loader intentionally accepts a position-independent,
long-mode raw image no larger than one 4 KiB page. External UEFI applications
use a separate PE/COFF loader and can occupy a larger guest address space.
Neither path is a general OS disk mount mechanism; virtual block devices and
broader UEFI boot-service support remain future work.

## External UEFI Guest Milestone

External boot disks are staged locally in `guests/local/`. Disk files in that
directory are gitignored and are never modified by Ferrovisor. To stage the
RustOS POC image without changing its source copy, use:

```powershell
Copy-Item C:\Users\tim\RustroverProjects\OS\dist\rustos-poc.img .\guests\local\rustos-poc.img
```

During each build, Ferrovisor discovers `guests/local/*.img`, validates the GPT
and FAT32 layout, extracts `EFI/BOOT/BOOTX64.EFI`, and adds the disk to the VM
manager as an imported UEFI disk. The build fails with a specific error if an
image is not a GPT/FAT32 UEFI disk with that standard boot path.

Milestone progress:

- Complete: ignored local disk staging, GPT/FAT32 discovery, EFI application
  extraction, generated catalog entries, PE/COFF relocation/loading, larger
  guest mappings, a UEFI launch trampoline, and serial-backed text
  console/input protocols.
- Next: virtual block devices and broader UEFI boot-service support.
- Later: interrupts and device emulation for general-purpose operating systems.

Select an imported UEFI disk from the VM manager to launch its
`EFI/BOOT/BOOTX64.EFI` application. The current UEFI environment is intentionally
small: applications that require block I/O, filesystem protocols, or other
unimplemented boot services are not supported yet.

## Troubleshooting

- **`AMD-V/SVM is not exposed`**: nested virtualization is off, the host CPU is
  not AMD, or VMware is running in Host VBS mode.
- **`VM_CR.SVMDIS`**: firmware or the outer hypervisor locked SVM. Check UEFI
  firmware virtualization settings and VMware configuration.
- **`invalid guest state`**: preserve the printed info values and VMware log;
  VMCB validation differed from the expected AMD SVM contract.
- **No keyboard input**: click inside the VMware console. Only common US set-1
  keys are mapped in this milestone.

## Linux guest roadmap

The next coherent increment is a direct Linux `bzImage` boot path with a serial
console. It requires: guest RAM ranges, Linux boot protocol structures, an
initramfs, local APIC/PIT or paravirtual clock support, interrupt injection,
and enough port/MMIO emulation for early boot. Virtio-console and virtio-blk can
follow once interrupt delivery is reliable.

## References

- [AMD64 Architecture Programmer's Manual, Volume 2 (publication 24593)](https://docs.amd.com/v/u/en-US/24593_3.45_APM_Vol2), SVM chapter and VMCB layout appendix
- [Rust `x86_64-unknown-none` platform documentation](https://doc.rust-lang.org/rustc/platform-support/x86_64-unknown-none.html)
- [rust-osdev `bootloader` 0.11 documentation](https://docs.rs/bootloader/0.11.17/bootloader/)
- [VMware Workstation Host VBS mode limitations](https://techdocs.broadcom.com/us/en/vmware-cis/desktop-hypervisors/workstation-pro/17-0/using-vmware-workstation-pro/running-workstation-on-a-hyper-v-enabled-host/limitations-of-host-vbs-mode.html)

## License

Dual-licensed under MIT or Apache-2.0 at your option.
