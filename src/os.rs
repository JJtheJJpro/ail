pub fn info() -> String {
    format!("{}", os_info::get())
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
#[derive(Debug)]
pub struct AcpiTable {
    pub signature: [u8; 4],
    pub data: Vec<u8>,
}

#[cfg(target_os = "linux")]
pub fn get_acpi_tables() -> std::io::Result<Vec<AcpiTable>> {
    use std::fs;
    use std::path::Path;

    fn collect_tables(path: &Path, tables: &mut Vec<AcpiTable>) -> std::io::Result<()> {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let entry_path = entry.path();
            let file_type = entry.file_type()?;

            if file_type.is_dir() {
                collect_tables(&entry_path, tables)?;
            } else if file_type.is_file() {
                let data = fs::read(entry_path)?;
                let signature = data
                    .get(..4)
                    .ok_or_else(|| {
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "ACPI table is shorter than its signature",
                        )
                    })?
                    .try_into()
                    .expect("slice length was checked");
                tables.push(AcpiTable { signature, data });
            }
        }

        Ok(())
    }

    let mut tables = Vec::new();
    collect_tables(Path::new("/sys/firmware/acpi/tables"), &mut tables)?;
    Ok(tables)
}

#[cfg(target_os = "windows")]
pub fn get_acpi_tables() -> std::io::Result<Vec<AcpiTable>> {
    use std::collections::HashSet;
    use std::io;
    use std::mem::size_of;
    use std::ptr::null_mut;

    const ACPI_PROVIDER: u32 = u32::from_be_bytes(*b"ACPI");

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn EnumSystemFirmwareTables(provider: u32, buffer: *mut u8, buffer_size: u32) -> u32;
        fn GetSystemFirmwareTable(
            provider: u32,
            table_id: u32,
            buffer: *mut u8,
            buffer_size: u32,
        ) -> u32;
    }

    let mut size = unsafe { EnumSystemFirmwareTables(ACPI_PROVIDER, null_mut(), 0) };
    if size == 0 {
        return Err(io::Error::last_os_error());
    }

    let table_ids = loop {
        let mut buffer = vec![0u8; size as usize];
        let written = unsafe { EnumSystemFirmwareTables(ACPI_PROVIDER, buffer.as_mut_ptr(), size) };
        if written == 0 {
            return Err(io::Error::last_os_error());
        }
        if written > size {
            size = written;
            continue;
        }

        buffer.truncate(written as usize);
        if buffer.len() % size_of::<u32>() != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Windows returned a malformed ACPI table list",
            ));
        }

        break buffer
            .chunks_exact(size_of::<u32>())
            .map(|bytes| u32::from_ne_bytes(bytes.try_into().expect("chunk size is four bytes")))
            .collect::<Vec<_>>();
    };

    let mut tables = Vec::with_capacity(table_ids.len());
    let mut seen_table_ids = HashSet::new();
    for table_id in table_ids {
        // Windows enumerates duplicate signatures but only retrieves the first matching table.
        if !seen_table_ids.insert(table_id) {
            continue;
        }

        let mut size = unsafe { GetSystemFirmwareTable(ACPI_PROVIDER, table_id, null_mut(), 0) };
        if size == 0 {
            return Err(io::Error::last_os_error());
        }

        let data = loop {
            let mut buffer = vec![0u8; size as usize];
            let written = unsafe {
                GetSystemFirmwareTable(ACPI_PROVIDER, table_id, buffer.as_mut_ptr(), size)
            };
            if written == 0 {
                return Err(io::Error::last_os_error());
            }
            if written > size {
                size = written;
                continue;
            }

            buffer.truncate(written as usize);
            break buffer;
        };

        let signature = data
            .get(..4)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Windows returned an ACPI table shorter than its signature",
                )
            })?
            .try_into()
            .expect("slice length was checked");
        tables.push(AcpiTable { signature, data });
    }

    Ok(tables)
}
