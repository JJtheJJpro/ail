#[cfg(target_arch = "x86")]
use core::arch::asm;

#[cfg(target_arch = "x86_64")]
pub fn has_cpuid() -> bool {
    true
}

#[cfg(target_arch = "x86")]
pub fn has_cpuid() -> bool {
    let orig: u32;
    let flipped: u32;
    unsafe {
        asm!(
            "pushfd",
            "pop {orig:e}",
            "mov {tmp:e}, {orig:e}",
            "xor {tmp:e}, 0x200000",
            "push {tmp:e}",
            "popfd",
            "pushfd",
            "pop {tmp:e}",
            "push {orig:e}",
            "popfd",
            orig = out(reg) orig,
            tmp = out(reg) flipped,
        );
    }
    (orig ^ flipped) & 0x20_0000 != 0
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
pub fn has_cpuid() -> bool {
    false
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArmCpuId {
    pub implementer: u8,
    pub variant: u8,
    pub architecture: u8,
    pub part: u16,
    pub revision: u8,
}

pub const fn decode_arm_midr(midr: u64) -> ArmCpuId {
    ArmCpuId {
        implementer: (midr >> 24) as u8,
        variant: ((midr >> 20) & 0xf) as u8,
        architecture: ((midr >> 16) & 0xf) as u8,
        part: ((midr >> 4) & 0xfff) as u16,
        revision: (midr & 0xf) as u8,
    }
}

pub const fn arm_cpu_name(id: ArmCpuId) -> &'static str {
    match (id.implementer, id.part) {
        (0x41, 0xd03) => "Cortex-A53",
        (0x41, 0xd04) => "Cortex-A35",
        (0x41, 0xd05) => "Cortex-A55",
        (0x41, 0xd07) => "Cortex-A57",
        (0x41, 0xd08) => "Cortex-A72",
        (0x41, 0xd09) => "Cortex-A73",
        (0x41, 0xd0a) => "Cortex-A75",
        (0x41, 0xd0b) => "Cortex-A76",
        (0x41, 0xd0d) => "Cortex-A77",
        (0x41, 0xd41) => "Cortex-A78",
        (0x41, 0xd44) => "Cortex-X1",
        (0x41, 0xd46) => "Cortex-A510",
        (0x41, 0xd47) => "Cortex-A710",
        (0x41, 0xd4e) => "Cortex-X3",
        (0x41, 0xd4f) => "Cortex-A720",
        (0x41, 0xd80) => "Neoverse-V1",
        (0x41, 0xd81) => "Neoverse-N2",
        (0x61, _) => "Apple CPU (part-specific)",
        _ => "unknown ARM CPU",
    }
}

#[cfg(feature = "std")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CpuFamilyInfo {
    pub model: Option<String>,
    pub vendor: Option<String>,
    pub physical_cores: Option<usize>,
    pub logical_processors: Option<usize>,
}

#[cfg(feature = "std")]
impl core::fmt::Display for CpuFamilyInfo {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(
            self.model
                .as_deref()
                .or(self.vendor.as_deref())
                .unwrap_or("Unknown CPU"),
        )?;

        match (self.physical_cores, self.logical_processors) {
            (Some(cores), Some(logical)) => {
                write!(f, " ({cores} cores, {logical} logical processors)")
            }
            (Some(cores), None) => write!(f, " ({cores} cores)"),
            (None, Some(logical)) => write!(f, " ({logical} logical processors)"),
            (None, None) => Ok(()),
        }
    }
}

#[cfg(feature = "std")]
pub fn info() -> CpuFamilyInfo {
    #[allow(unused_mut)]
    let mut info = CpuFamilyInfo {
        model: None,
        vendor: None,
        physical_cores: None,
        logical_processors: std::thread::available_parallelism().ok().map(usize::from),
    };

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    detect_x86(&mut info);

    #[cfg(feature = "std")]
    {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        detect_linux(&mut info);

        #[cfg(target_os = "windows")]
        detect_windows(&mut info);

        #[cfg(target_os = "macos")]
        detect_macos(&mut info);
    }

    info
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn detect_x86(info: &mut CpuFamilyInfo) {
    if !has_cpuid() {
        return;
    }

    let cpuid = raw_cpuid::CpuId::new();
    let vendor_id = cpuid
        .get_vendor_info()
        .map(|vendor| vendor.as_str().to_owned());
    info.vendor = vendor_id.as_deref().map(|vendor| match vendor {
        "GenuineIntel" => "Intel".to_owned(),
        "AuthenticAMD" => "AMD".to_owned(),
        other => other.to_owned(),
    });
    info.model = cpuid
        .get_processor_brand_string()
        .map(|brand| brand.as_str().trim().to_owned())
        .filter(|brand| !brand.is_empty());

    let mut core_level_processors = None;
    let mut smt_level_processors = None;
    if let Some(levels) = cpuid
        .get_extended_topology_info_v2()
        .or_else(|| cpuid.get_extended_topology_info())
    {
        for level in levels {
            match level.level_type() {
                raw_cpuid::TopologyType::SMT => {
                    smt_level_processors = Some(usize::from(level.processors()));
                }
                raw_cpuid::TopologyType::Core => {
                    core_level_processors = Some(usize::from(level.processors()));
                }
                _ => {}
            }
        }
    }
    info.logical_processors = core_level_processors.or(info.logical_processors);

    if vendor_id.as_deref() == Some("GenuineIntel") {
        info.physical_cores = cpuid
            .get_cache_parameters()
            .and_then(|mut caches| {
                caches.find(|cache| cache.cache_type() != raw_cpuid::CacheType::Null)
            })
            .map(|cache| cache.max_cores_for_package())
            .filter(|&cores| core_level_processors.is_none_or(|logical| cores <= logical));
    } else if vendor_id.as_deref() == Some("AuthenticAMD") {
        info.physical_cores = core_level_processors
            .zip(smt_level_processors)
            .filter(|(_, threads_per_core)| *threads_per_core != 0)
            .map(|(logical, threads_per_core)| logical / threads_per_core);
    }
}

#[cfg(all(feature = "std", any(target_os = "linux", target_os = "android")))]
fn detect_linux(info: &mut CpuFamilyInfo) {
    use std::collections::HashSet;
    use std::fs;
    use std::path::Path;

    let cpuinfo = fs::read_to_string("/proc/cpuinfo").ok();
    if let Some(cpuinfo) = cpuinfo.as_deref() {
        let mut processor = None;
        let mut hardware = None;
        let mut implementer = None;
        let mut part = None;

        for line in cpuinfo.lines().take_while(|line| !line.trim().is_empty()) {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "model name" | "uarch" if info.model.is_none() => {
                    info.model = nonempty(value);
                }
                "Processor" => processor = nonempty(value),
                "Hardware" => hardware = nonempty(value),
                "CPU implementer" => implementer = parse_hex_u8(value),
                "CPU part" => part = parse_hex_u16(value),
                "vendor_id" if info.vendor.is_none() => info.vendor = nonempty(value),
                _ => {}
            }
        }

        if let (Some(implementer), Some(part)) = (implementer, part) {
            let id = ArmCpuId {
                implementer,
                variant: 0,
                architecture: 0,
                part,
                revision: 0,
            };
            let core_name = arm_cpu_name(id);
            if core_name != "unknown ARM CPU" {
                let model = if implementer == 0x61 {
                    core_name.to_owned()
                } else {
                    format!("{} {core_name}", arm_vendor_name(implementer))
                };
                info.model.get_or_insert(model);
            }
            info.vendor
                .get_or_insert_with(|| arm_vendor_name(implementer).to_owned());
            if core_name == "unknown ARM CPU" {
                info.model.get_or_insert_with(|| {
                    format!("{} CPU (part 0x{part:03x})", arm_vendor_name(implementer))
                });
            }
        }

        if info.model.is_none() {
            info.model = processor
                .filter(|name| !is_generic_processor_name(name))
                .or(hardware);
        }
    }

    let root = Path::new("/sys/devices/system/cpu");
    let online_count = fs::read_to_string(root.join("online"))
        .ok()
        .and_then(|value| parse_cpu_list(&value));
    let mut cpu_directory_count = 0usize;
    let mut cores = HashSet::new();

    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let Some(file_name) = file_name.to_str() else {
                continue;
            };
            if file_name.strip_prefix("cpu").is_some_and(|number| {
                !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit())
            }) {
                cpu_directory_count += 1;
                let topology = entry.path().join("topology");
                let core_id = fs::read_to_string(topology.join("core_id"))
                    .ok()
                    .and_then(|value| value.trim().parse::<u32>().ok());
                let package_id = fs::read_to_string(topology.join("physical_package_id"))
                    .ok()
                    .and_then(|value| value.trim().parse::<u32>().ok());
                if let (Some(package_id), Some(core_id)) = (package_id, core_id) {
                    cores.insert((package_id, core_id));
                }
            }
        }
    }

    info.logical_processors = online_count
        .or_else(|| (cpu_directory_count != 0).then_some(cpu_directory_count))
        .or(info.logical_processors);
    if !cores.is_empty() {
        info.physical_cores = Some(cores.len());
    }
}

#[cfg(all(feature = "std", any(target_os = "linux", target_os = "android")))]
fn nonempty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

#[cfg(all(feature = "std", any(target_os = "linux", target_os = "android")))]
fn parse_hex_u8(value: &str) -> Option<u8> {
    u8::from_str_radix(value.trim().trim_start_matches("0x"), 16).ok()
}

#[cfg(all(feature = "std", any(target_os = "linux", target_os = "android")))]
fn parse_hex_u16(value: &str) -> Option<u16> {
    u16::from_str_radix(value.trim().trim_start_matches("0x"), 16).ok()
}

#[cfg(all(feature = "std", any(target_os = "linux", target_os = "android")))]
fn arm_vendor_name(implementer: u8) -> &'static str {
    match implementer {
        0x41 => "Arm",
        0x42 => "Broadcom",
        0x43 => "Cavium",
        0x48 => "HiSilicon",
        0x4e => "NVIDIA",
        0x51 => "Qualcomm",
        0x53 => "Samsung",
        0x56 => "Marvell",
        0x61 => "Apple",
        0x69 => "Intel",
        0xc0 => "Ampere",
        _ => "ARM-compatible",
    }
}

#[cfg(all(feature = "std", any(target_os = "linux", target_os = "android")))]
fn is_generic_processor_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.contains("processor rev") || name.contains("aarch64 processor")
}

#[cfg(all(feature = "std", any(test, target_os = "linux", target_os = "android")))]
fn parse_cpu_list(value: &str) -> Option<usize> {
    let mut count = 0usize;
    for range in value.trim().split(',') {
        let (start, end) = match range.split_once('-') {
            Some((start, end)) => (start.parse::<usize>().ok()?, end.parse::<usize>().ok()?),
            None => {
                let cpu = range.parse::<usize>().ok()?;
                (cpu, cpu)
            }
        };
        if end < start {
            return None;
        }
        count = count.checked_add(end.checked_sub(start)?.checked_add(1)?)?;
    }
    (count != 0).then_some(count)
}

#[cfg(all(feature = "std", target_os = "windows"))]
fn detect_windows(info: &mut CpuFamilyInfo) {
    use std::ffi::c_void;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetActiveProcessorCount(group_number: u16) -> u32;
        fn GetLogicalProcessorInformationEx(
            relationship_type: u32,
            buffer: *mut u8,
            returned_length: *mut u32,
        ) -> i32;
    }

    const ALL_PROCESSOR_GROUPS: u16 = 0xffff;
    const RELATION_PROCESSOR_CORE: u32 = 0;
    let active_processors = unsafe { GetActiveProcessorCount(ALL_PROCESSOR_GROUPS) };
    if active_processors != 0 {
        info.logical_processors = Some(active_processors as usize);
    }

    let mut buffer_size = 0u32;
    let mut core_count = None;
    unsafe {
        GetLogicalProcessorInformationEx(
            RELATION_PROCESSOR_CORE,
            core::ptr::null_mut(),
            &mut buffer_size,
        );
    }
    while buffer_size != 0 {
        let mut buffer = vec![0u8; buffer_size as usize];
        let mut returned_length = buffer_size;
        let succeeded = unsafe {
            GetLogicalProcessorInformationEx(
                RELATION_PROCESSOR_CORE,
                buffer.as_mut_ptr(),
                &mut returned_length,
            )
        };
        if succeeded != 0 {
            let mut offset = 0usize;
            let mut cores = 0usize;
            let data_length = returned_length as usize;
            let mut valid = data_length <= buffer.len();
            while valid && offset < data_length {
                if data_length - offset < 8 {
                    valid = false;
                    break;
                }
                let relationship =
                    u32::from_ne_bytes(buffer[offset..offset + 4].try_into().unwrap());
                let record_size =
                    u32::from_ne_bytes(buffer[offset + 4..offset + 8].try_into().unwrap()) as usize;
                if record_size < 8 || record_size > data_length - offset {
                    valid = false;
                    break;
                }
                if relationship == RELATION_PROCESSOR_CORE {
                    cores += 1;
                }
                offset += record_size;
            }
            if valid && offset == data_length && cores != 0 {
                core_count = Some(cores);
            }
            break;
        }
        if returned_length <= buffer_size {
            break;
        }
        buffer_size = returned_length;
    }
    info.physical_cores = core_count.or(info.physical_cores);

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn RegGetValueW(
            key: *mut c_void,
            sub_key: *const u16,
            value: *const u16,
            flags: u32,
            value_type: *mut u32,
            data: *mut c_void,
            data_size: *mut u32,
        ) -> i32;
    }

    const HKEY_LOCAL_MACHINE: *mut c_void = 0x8000_0002u32 as i32 as isize as *mut c_void;
    const RRF_RT_REG_SZ: u32 = 0x0000_0002;
    let sub_key: Vec<u16> = "HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let value: Vec<u16> = "ProcessorNameString"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut size = 0u32;
    let result = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            sub_key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            &mut size,
        )
    };
    if result != 0 || size < 2 {
        return;
    }

    let mut data = vec![0u16; size as usize / 2];
    let result = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            sub_key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            core::ptr::null_mut(),
            data.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if result == 0 {
        let length = data
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(data.len());
        let model = String::from_utf16_lossy(&data[..length]);
        let model = model.trim();
        if !model.is_empty() {
            info.model = Some(model.to_owned());
        }
    }
}

#[cfg(all(feature = "std", target_os = "macos"))]
fn detect_macos(info: &mut CpuFamilyInfo) {
    use std::ffi::{CStr, c_char, c_int, c_void};

    #[link(name = "System")]
    unsafe extern "C" {
        fn sysctlbyname(
            name: *const c_char,
            old_value: *mut c_void,
            old_length: *mut usize,
            new_value: *mut c_void,
            new_length: usize,
        ) -> c_int;
    }

    fn sysctl_string(name: &'static CStr) -> Option<String> {
        let mut size = 0usize;
        let result = unsafe {
            sysctlbyname(
                name.as_ptr(),
                core::ptr::null_mut(),
                &mut size,
                core::ptr::null_mut(),
                0,
            )
        };
        if result != 0 || size == 0 {
            return None;
        }

        let mut bytes = vec![0u8; size];
        let result = unsafe {
            sysctlbyname(
                name.as_ptr(),
                bytes.as_mut_ptr().cast(),
                &mut size,
                core::ptr::null_mut(),
                0,
            )
        };
        if result != 0 {
            return None;
        }
        bytes.truncate(size);
        let end = bytes
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(bytes.len());
        let value = String::from_utf8_lossy(&bytes[..end]).trim().to_owned();
        (!value.is_empty()).then_some(value)
    }

    fn sysctl_cpu_count(name: &'static CStr) -> Option<usize> {
        let mut count = 0i32;
        let mut size = core::mem::size_of::<i32>();
        let result = unsafe {
            sysctlbyname(
                name.as_ptr(),
                (&mut count as *mut i32).cast(),
                &mut size,
                core::ptr::null_mut(),
                0,
            )
        };
        (result == 0 && count > 0).then_some(count as usize)
    }

    let brand_name = CStr::from_bytes_with_nul(b"machdep.cpu.brand_string\0")
        .expect("static sysctl name is nul terminated");
    info.model = sysctl_string(brand_name).or_else(|| {
        #[cfg(target_arch = "aarch64")]
        {
            Some("Apple Silicon".to_owned())
        }
        #[cfg(not(target_arch = "aarch64"))]
        {
            None
        }
    });

    let physical = CStr::from_bytes_with_nul(b"hw.physicalcpu\0")
        .expect("static sysctl name is nul terminated");
    let logical = CStr::from_bytes_with_nul(b"hw.logicalcpu\0")
        .expect("static sysctl name is nul terminated");
    info.physical_cores = sysctl_cpu_count(physical).or(info.physical_cores);
    info.logical_processors = sysctl_cpu_count(logical).or(info.logical_processors);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_arm_midr_fields_and_name() {
        let id = decode_arm_midr(0x410f_d030);
        assert_eq!(id.implementer, 0x41);
        assert_eq!(id.variant, 0);
        assert_eq!(id.architecture, 0xf);
        assert_eq!(id.part, 0xd03);
        assert_eq!(id.revision, 0);
        assert_eq!(arm_cpu_name(id), "Cortex-A53");
    }

    #[test]
    fn counts_linux_cpu_ranges() {
        assert_eq!(parse_cpu_list("0-3,8,10-11"), Some(7));
        assert_eq!(parse_cpu_list("3-1"), None);
        assert_eq!(parse_cpu_list(""), None);
    }
}
