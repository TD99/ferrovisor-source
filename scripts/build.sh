#!/usr/bin/env bash
set -euo pipefail

root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

if [[ $# -eq 0 ]]; then
    target="baremetal"
elif [[ $# -eq 2 && $1 == "--target" ]]; then
    target="$2"
else
    printf '%s\n' "usage: ./scripts/build.sh [--target baremetal|vmware|hyperv]" >&2
    exit 1
fi

case "$target" in
    baremetal|vmware|hyperv) ;;
    *)
        printf '%s\n' "error: --target must be baremetal, vmware, or hyperv" >&2
        exit 1
        ;;
esac

for tool in rustup cargo "${CC:-cc}" "${AR:-ar}"; do
    command -v "$tool" >/dev/null 2>&1 || {
        printf 'error: %s is required\n' "$tool" >&2
        exit 1
    }
done
if [[ $target == "hyperv" ]]; then
    command -v qemu-img >/dev/null 2>&1 || {
        printf '%s\n' "error: qemu-img is required for the hyperv target" >&2
        exit 1
    }
fi

mkdir -p target/uefi-shim
"${CC:-cc}" -c -O2 -ffreestanding -fno-stack-protector build/uefi_shim.c -o target/uefi-shim/uefi_shim.o
"${AR:-ar}" rcs target/uefi-shim/libferrovisor_uefi_shim.a target/uefi-shim/uefi_shim.o

rustup show active-toolchain
cargo_home="$root/target/cargo-home"
mkdir -p "$cargo_home"
printf '[target.x86_64-unknown-uefi]\nrustflags = ["-Lnative=%s", "-lstatic=ferrovisor_uefi_shim"]\n' \
    "$root/target/uefi-shim" > "$cargo_home/config.toml"
export CARGO_HOME="$cargo_home"
cargo run --release -- --target "$target"

images=(dist/ferrovisor-bios.img dist/ferrovisor-uefi.img)
if [[ $target == "vmware" ]]; then
    images+=(dist/ferrovisor-bios.vmdk dist/ferrovisor-bios.vmx)
    images+=(dist/ferrovisor-uefi.vmdk dist/ferrovisor-uefi.vmx)
elif [[ $target == "hyperv" ]]; then
    images+=(dist/ferrovisor-bios.vhdx dist/ferrovisor-uefi.vhdx)
fi

for image in "${images[@]}"; do
    [[ -f "$image" ]] || {
        printf 'error: build completed without required artifact: %s\n' "$image" >&2
        exit 1
    }
done

printf '%s\n' "Images are ready in ./dist"
if [[ $target == "vmware" ]]; then
    printf '%s\n' "Open ./dist/ferrovisor-bios.vmx in VMware Workstation."
elif [[ $target == "hyperv" ]]; then
    printf '%s\n' "Attach ./dist/ferrovisor-bios.vhdx to a Hyper-V Generation 1 VM."
fi
