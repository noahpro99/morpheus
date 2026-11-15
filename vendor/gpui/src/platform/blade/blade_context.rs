use anyhow::Context as _;
use blade_graphics as gpu;
use std::sync::Arc;
use util::ResultExt;

#[cfg_attr(target_os = "macos", derive(Clone))]
pub struct BladeContext {
    pub(super) gpu: Arc<gpu::Context>,
}

impl BladeContext {
    pub fn new() -> anyhow::Result<Self> {
        Self::for_display(None)
    }

    /// Morpheus patch: on Linux, with the Wayland display (when there is
    /// one) so the GPU chosen is one that presents to it; see
    /// `device_choice`.
    pub fn for_display(
        #[allow(unused_variables)] wayland_display: Option<*mut std::ffi::c_void>,
    ) -> anyhow::Result<Self> {
        let device_id_forced = match std::env::var("ZED_DEVICE_ID") {
            Ok(val) => parse_pci_id(&val)
                .context("Failed to parse device ID from `ZED_DEVICE_ID` environment variable")
                .log_err(),
            Err(std::env::VarError::NotPresent) => None,
            err => {
                err.context("Failed to read value of `ZED_DEVICE_ID` environment variable")
                    .log_err();
                None
            }
        };
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        let device_id_chosen = device_id_forced.or_else(|| choose_device(wayland_display));
        #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
        let device_id_chosen = device_id_forced;
        let init = |device_id: u32| unsafe {
            gpu::Context::init(gpu::ContextDesc {
                presentation: true,
                // Morpheus patch (issue #76): MORPHEUS_GPU_VALIDATION=1 turns on
                // Vulkan's validation layer (in the dev shell) and blade's crash
                // markers, which name the last pass the GPU ran when it is lost.
                validation: std::env::var_os("MORPHEUS_GPU_VALIDATION").is_some_and(|v| v == "1"),
                device_id,
                ..Default::default()
            })
        };
        let context = match (init(device_id_chosen.unwrap_or(0)), device_id_chosen) {
            // Morpheus patch: the device picked would not start; Blade's own pick.
            (Err(err), Some(id)) if device_id_forced.is_none() => {
                log::warn!("GPU 0x{id:04x} could not be used ({err:?}); trying the others");
                init(0)
            }
            (result, _) => result,
        };
        let gpu = Arc::new(context.map_err(|e| anyhow::anyhow!("{e:?}"))?);
        let info = gpu.device_information();
        log::info!(
            target: "morpheus_gpu",
            "GPU: drawing with {} (driver {}, {})",
            info.device_name,
            info.driver_name,
            info.driver_info
        );
        crate::update_gpu_info(|gpu_info| {
            gpu_info.drawing = Some((
                info.device_name.clone(),
                info.driver_name.clone(),
                info.driver_info.clone(),
            ));
            gpu_info.validation = std::env::var_os("MORPHEUS_GPU_VALIDATION").is_some_and(|v| v == "1");
        });
        Ok(Self { gpu })
    }
}

/// Morpheus patch: the PCI device id of the GPU to draw with (see
/// `device_choice`), logged with the others Vulkan lists.
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn choose_device(wayland_display: Option<*mut std::ffi::c_void>) -> Option<u32> {
    use super::device_choice::{LOG, choose, devices, wanted};
    let devices = match devices(wayland_display) {
        Ok(devices) => devices,
        Err(err) => {
            log::warn!(target: LOG, "GPU: could not list the Vulkan devices: {err}");
            return None;
        }
    };
    crate::update_gpu_info(|info| {
        info.devices = devices.iter().map(super::device_choice::Device::describe).collect();
    });
    for device in &devices {
        log::info!(target: LOG, "GPU found: {device}");
    }
    let wanted = wanted();
    let chosen = choose(&devices, wanted)?;
    log::info!(target: LOG, "GPU chosen ({wanted:?} wanted): {chosen}");
    Some(chosen.device_id)
}

fn parse_pci_id(id: &str) -> anyhow::Result<u32> {
    let mut id = id.trim();

    if id.starts_with("0x") || id.starts_with("0X") {
        id = &id[2..];
    }
    let is_hex_string = id.chars().all(|c| c.is_ascii_hexdigit());
    let is_4_chars = id.len() == 4;
    anyhow::ensure!(
        is_4_chars && is_hex_string,
        "Expected a 4 digit PCI ID in hexadecimal format"
    );

    u32::from_str_radix(id, 16).context("parsing PCI ID as hex")
}

#[cfg(test)]
mod tests {
    use super::parse_pci_id;

    #[test]
    fn test_parse_device_id() {
        assert!(parse_pci_id("0xABCD").is_ok());
        assert!(parse_pci_id("ABCD").is_ok());
        assert!(parse_pci_id("abcd").is_ok());
        assert!(parse_pci_id("1234").is_ok());
        assert!(parse_pci_id("123").is_err());
        assert_eq!(
            parse_pci_id(&format!("{:x}", 0x1234)).unwrap(),
            parse_pci_id(&format!("{:X}", 0x1234)).unwrap(),
        );

        assert_eq!(
            parse_pci_id(&format!("{:#x}", 0x1234)).unwrap(),
            parse_pci_id(&format!("{:#X}", 0x1234)).unwrap(),
        );
        assert_eq!(
            parse_pci_id(&format!("{:#x}", 0x1234)).unwrap(),
            parse_pci_id(&format!("{:#X}", 0x1234)).unwrap(),
        );
    }
}
