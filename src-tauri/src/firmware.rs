use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

fn pico_uf2(data: &[u8]) -> bool {
    if data.is_empty() || data.len() > 8 * 1024 * 1024 || !data.len().is_multiple_of(512) {
        return false;
    }
    data.chunks_exact(512).all(|block| {
        let word = |offset| u32::from_le_bytes(block[offset..offset + 4].try_into().unwrap());
        word(0) == 0x0a32_4655
            && word(4) == 0x9e5d_5157
            && word(508) == 0x0ab1_6f30
            && word(8) & 0x2000 != 0
            && word(28) == 0xe48b_ff56
            && word(16) <= 476
    })
}

/// Export only the bundled Pico image, with exclusive creation so existing downloads survive.
pub fn export(resources: &Path, downloads: &Path) -> Result<PathBuf, String> {
    let data = fs::read(resources.join("resources/firmware/osg-pico-gateway.uf2"))
        .map_err(|_| "The Pico firmware is not included in this build. Run pnpm pico:build before rebuilding Cockpit.".to_string())?;
    if !pico_uf2(&data) {
        return Err("The bundled Pico firmware is not a valid RP2040 UF2 image.".into());
    }
    fs::create_dir_all(downloads).map_err(|error| format!("Could not open Downloads: {error}"))?;
    for suffix in 0..1000 {
        let name = if suffix == 0 {
            "osg-pico-gateway.uf2".into()
        } else {
            format!("osg-pico-gateway ({suffix}).uf2")
        };
        let path = downloads.join(name);
        let mut file = match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("Could not save the firmware: {error}")),
        };
        if let Err(error) = file.write_all(&data).and_then(|_| file.sync_all()) {
            drop(file);
            let _ = fs::remove_file(&path);
            return Err(format!("Could not save the firmware: {error}"));
        }
        return Ok(path);
    }
    Err("Downloads already contains too many copies of the Pico firmware.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn export_preserves_existing_files_and_rejects_invalid_firmware() {
        let root = std::env::temp_dir().join(format!("osg-export-{}", uuid::Uuid::new_v4()));
        let firmware = root.join("resources/firmware");
        let downloads = root.join("Downloads");
        fs::create_dir_all(&firmware).unwrap();
        fs::create_dir_all(&downloads).unwrap();
        fs::write(downloads.join("osg-pico-gateway.uf2"), b"existing download").unwrap();
        let mut data = vec![0u8; 512];
        for (offset, word) in [
            (0, 0x0a32_4655u32),
            (4, 0x9e5d_5157),
            (8, 0x2000),
            (16, 256),
            (28, 0xe48b_ff56),
            (508, 0x0ab1_6f30),
        ] {
            data[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
        }
        fs::write(firmware.join("osg-pico-gateway.uf2"), &data).unwrap();
        let path = export(&root, &downloads).unwrap();
        assert_eq!(path.file_name().unwrap(), "osg-pico-gateway (1).uf2");
        assert_eq!(fs::read(path).unwrap(), data);
        assert_eq!(
            fs::read(downloads.join("osg-pico-gateway.uf2")).unwrap(),
            b"existing download"
        );
        fs::write(firmware.join("osg-pico-gateway.uf2"), b"bad image").unwrap();
        assert!(export(&root, &downloads).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
