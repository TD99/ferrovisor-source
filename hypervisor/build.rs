use std::{
    env,
    fs,
    path::{Path, PathBuf},
};

const SECTOR_SIZE: usize = 512;

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest path missing"));
    let images_dir = manifest.join("..").join("guests").join("local");
    println!("cargo:rerun-if-changed={}", images_dir.display());

    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR missing"));
    let extracted = out.join("external-guests");
    fs::create_dir_all(&extracted).expect("failed to create external guest output directory");

    let mut entries = Vec::new();
    if images_dir.exists() {
        let mut paths = fs::read_dir(&images_dir)
            .expect("failed to read local guest directory")
            .map(|entry| entry.expect("failed to read local guest entry").path())
            .filter(|path| path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("img")))
            .collect::<Vec<_>>();
        paths.sort();

        for path in paths {
            println!("cargo:rerun-if-changed={}", path.display());
            let name = path
                .file_stem()
                .and_then(|name| name.to_str())
                .expect("external guest image must have a UTF-8 filename");
            let efi = extract_bootx64(&path).unwrap_or_else(|error| {
                panic!("failed to import external guest {}: {error}", path.display())
            });
            let destination = extracted.join(format!("{name}.efi"));
            fs::write(&destination, efi).expect("failed to write extracted UEFI image");
            entries.push((name.to_owned(), destination));
        }
    }

    let mut generated = String::from("pub static EXTERNAL_UEFI_IMAGES: &[ExternalUefiImage] = &[\n");
    for (name, path) in entries {
        generated.push_str(&format!(
            "    ExternalUefiImage {{ name: {name:?}, efi: include_bytes!({path:?}) }},\n",
            path = path.display().to_string(),
        ));
    }
    generated.push_str("];\n");
    fs::write(out.join("external_guests.rs"), generated).expect("failed to write external guest catalog");
}

fn extract_bootx64(path: &Path) -> Result<Vec<u8>, String> {
    let image = fs::read(path).map_err(|error| error.to_string())?;
    let partition = gpt_partition_start(&image)?;
    let fat = Fat32::open(&image, partition)?;
    let efi = fat.find_in_directory(fat.root_cluster, b"EFI        ")?;
    let boot = fat.find_in_directory(efi.cluster, b"BOOT       ")?;
    let app = fat.find_in_directory(boot.cluster, b"BOOTX64 EFI")?;
    if app.attributes & 0x10 != 0 {
        return Err("EFI/BOOT/BOOTX64.EFI is a directory".into());
    }
    fat.read_file(app.cluster, app.size as usize)
}

fn gpt_partition_start(image: &[u8]) -> Result<u64, String> {
    let header = sector(image, 1)?;
    if &header[..8] != b"EFI PART" {
        return Err("expected a GPT disk with an EFI PART header".into());
    }
    let entries_lba = u64_at(header, 72)?;
    let count = u32_at(header, 80)? as usize;
    let entry_size = u32_at(header, 84)? as usize;
    if entry_size < 56 {
        return Err("GPT partition entries are too small".into());
    }
    for index in 0..count {
        let offset = entries_lba as usize * SECTOR_SIZE + index * entry_size;
        let entry = image.get(offset..offset + entry_size).ok_or("GPT entry is outside image")?;
        if entry[..16].iter().any(|byte| *byte != 0) {
            return u64_at(entry, 32);
        }
    }
    Err("GPT contains no partitions".into())
}

struct DirectoryEntry {
    attributes: u8,
    cluster: u32,
    size: u32,
}

struct Fat32<'a> {
    image: &'a [u8],
    partition_lba: u64,
    bytes_per_sector: usize,
    sectors_per_cluster: usize,
    reserved_sectors: usize,
    fats: usize,
    fat_sectors: usize,
    root_cluster: u32,
}

impl<'a> Fat32<'a> {
    fn open(image: &'a [u8], partition_lba: u64) -> Result<Self, String> {
        let boot = sector(image, partition_lba)?;
        if &boot[82..90] != b"FAT32   " {
            return Err("first GPT partition is not FAT32".into());
        }
        let bytes_per_sector = u16_at(boot, 11)? as usize;
        if bytes_per_sector != SECTOR_SIZE || boot[13] == 0 {
            return Err("unsupported FAT32 sector or cluster size".into());
        }
        Ok(Self {
            image,
            partition_lba,
            bytes_per_sector,
            sectors_per_cluster: boot[13] as usize,
            reserved_sectors: u16_at(boot, 14)? as usize,
            fats: boot[16] as usize,
            fat_sectors: u32_at(boot, 36)? as usize,
            root_cluster: u32_at(boot, 44)?,
        })
    }

    fn find_in_directory(&self, cluster: u32, name: &[u8; 11]) -> Result<DirectoryEntry, String> {
        let mut cluster = cluster;
        loop {
            let bytes = self.cluster_bytes(cluster)?;
            for entry in bytes.chunks_exact(32) {
                if entry[0] == 0 {
                    return Err(format!("missing FAT32 entry {:?}", name));
                }
                if entry[0] == 0xE5 || entry[11] == 0x0F || &entry[..11] != name {
                    continue;
                }
                return Ok(DirectoryEntry {
                    attributes: entry[11],
                    cluster: ((u16_at(entry, 20)? as u32) << 16) | u16_at(entry, 26)? as u32,
                    size: u32_at(entry, 28)?,
                });
            }
            cluster = self.next_cluster(cluster)?;
            if cluster >= 0x0FFF_FFF8 {
                return Err(format!("missing FAT32 entry {:?}", name));
            }
        }
    }

    fn read_file(&self, mut cluster: u32, size: usize) -> Result<Vec<u8>, String> {
        let mut output = Vec::with_capacity(size);
        while output.len() < size {
            let bytes = self.cluster_bytes(cluster)?;
            let remaining = size - output.len();
            output.extend_from_slice(&bytes[..remaining.min(bytes.len())]);
            cluster = self.next_cluster(cluster)?;
            if output.len() < size && cluster >= 0x0FFF_FFF8 {
                return Err("FAT32 file ended before its advertised size".into());
            }
        }
        Ok(output)
    }

    fn cluster_bytes(&self, cluster: u32) -> Result<&'a [u8], String> {
        if cluster < 2 {
            return Err("invalid FAT32 cluster".into());
        }
        let data_lba = self.partition_lba
            + self.reserved_sectors as u64
            + (self.fats * self.fat_sectors) as u64
            + (cluster as u64 - 2) * self.sectors_per_cluster as u64;
        let start = data_lba as usize * self.bytes_per_sector;
        let length = self.sectors_per_cluster * self.bytes_per_sector;
        self.image.get(start..start + length).ok_or("FAT32 cluster is outside image".into())
    }

    fn next_cluster(&self, cluster: u32) -> Result<u32, String> {
        let fat_lba = self.partition_lba + self.reserved_sectors as u64;
        let offset = fat_lba as usize * self.bytes_per_sector + cluster as usize * 4;
        Ok(u32_at(self.image, offset)? & 0x0FFF_FFFF)
    }
}

fn sector(image: &[u8], lba: u64) -> Result<&[u8], String> {
    let start = lba as usize * SECTOR_SIZE;
    image.get(start..start + SECTOR_SIZE).ok_or("sector is outside image".into())
}

fn u16_at(bytes: &[u8], offset: usize) -> Result<u16, String> {
    let value = bytes.get(offset..offset + 2).ok_or("truncated integer")?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let value = bytes.get(offset..offset + 4).ok_or("truncated integer")?;
    Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

fn u64_at(bytes: &[u8], offset: usize) -> Result<u64, String> {
    let value = bytes.get(offset..offset + 8).ok_or("truncated integer")?;
    Ok(u64::from_le_bytes([
        value[0], value[1], value[2], value[3], value[4], value[5], value[6], value[7],
    ]))
}
