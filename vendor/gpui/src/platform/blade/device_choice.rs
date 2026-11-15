//! Morpheus patch: which Vulkan device draws the windows. Blade takes the
//! first device that works unless told a PCI device id, and on a laptop with
//! two GPUs the first is usually the integrated one (Intel before NVIDIA).
//! This picks the discrete GPU when there is one, unless the app asked for
//! the integrated one ([`crate::PREFER_INTEGRATED_GPU`]) or `MORPHEUS_GPU`
//! says `integrated` or `discrete`; `ZED_DEVICE_ID` still forces a device.
//! On Wayland a device that cannot present to the compositor is passed over,
//! and the log says why.

use ash::vk;
use std::ffi::c_void;

/// Logged under this target, which the app's log keeps at info.
pub(super) const LOG: &str = "morpheus_gpu";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Discrete,
    Integrated,
}

/// What the app or `MORPHEUS_GPU` asks for.
pub(super) fn wanted() -> Kind {
    match std::env::var("MORPHEUS_GPU").as_deref() {
        Ok("integrated") => Kind::Integrated,
        Ok("discrete") => Kind::Discrete,
        _ if crate::PREFER_INTEGRATED_GPU.load(std::sync::atomic::Ordering::Relaxed) => {
            Kind::Integrated
        }
        _ => Kind::Discrete,
    }
}

/// A device as Vulkan lists it.
#[derive(Clone, Debug)]
pub(super) struct Device {
    pub name: String,
    pub vendor_id: u32,
    pub device_id: u32,
    pub device_type: vk::PhysicalDeviceType,
    /// Whether it can present to the Wayland display (None: not asked).
    pub presents: Option<bool>,
}

impl Device {
    pub(super) fn describe(&self) -> crate::GpuDevice {
        crate::GpuDevice {
            name: self.name.clone(),
            vendor_id: self.vendor_id,
            device_id: self.device_id,
            kind: match self.device_type {
                vk::PhysicalDeviceType::DISCRETE_GPU => "discrete",
                vk::PhysicalDeviceType::INTEGRATED_GPU => "integrated",
                vk::PhysicalDeviceType::VIRTUAL_GPU => "virtual",
                vk::PhysicalDeviceType::CPU => "software",
                _ => "other",
            }
            .to_owned(),
            presents: self.presents,
        }
    }
}

impl std::fmt::Display for Device {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} (vendor 0x{:04x}, device 0x{:04x}, {:?})",
            self.name, self.vendor_id, self.device_id, self.device_type
        )
    }
}

/// The devices Vulkan lists, each with whether it presents to
/// `wayland_display` when one is given.
pub(super) fn devices(wayland_display: Option<*mut c_void>) -> anyhow::Result<Vec<Device>> {
    let entry = unsafe { ash::Entry::load() }?;
    let wayland = wayland_display.filter(|_| {
        let extensions =
            unsafe { entry.enumerate_instance_extension_properties(None) }.unwrap_or_default();
        let has = |name: &std::ffi::CStr| {
            extensions
                .iter()
                .any(|e| e.extension_name_as_c_str().is_ok_and(|n| n == name))
        };
        has(ash::khr::surface::NAME) && has(ash::khr::wayland_surface::NAME)
    });
    let extension_names = if wayland.is_some() {
        vec![
            ash::khr::surface::NAME.as_ptr(),
            ash::khr::wayland_surface::NAME.as_ptr(),
        ]
    } else {
        Vec::new()
    };
    let app_info = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_1);
    let create_info = vk::InstanceCreateInfo::default()
        .application_info(&app_info)
        .enabled_extension_names(&extension_names);
    let instance = unsafe { entry.create_instance(&create_info, None) }?;
    let wayland_surface =
        wayland.map(|_| ash::khr::wayland_surface::Instance::new(&entry, &instance));
    let listed = unsafe { instance.enumerate_physical_devices() };
    let devices = listed.map(|physical_devices| {
        physical_devices
            .into_iter()
            .map(|phd| {
                let props = unsafe { instance.get_physical_device_properties(phd) };
                // Blade draws and presents on queue family 0.
                let presents = match (&wayland_surface, wayland) {
                    (Some(surface), Some(display)) => Some(unsafe {
                        surface.get_physical_device_wayland_presentation_support(
                            phd,
                            0,
                            &mut *(display as *mut vk::wl_display),
                        )
                    }),
                    _ => None,
                };
                Device {
                    name: props
                        .device_name_as_c_str()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    vendor_id: props.vendor_id,
                    device_id: props.device_id,
                    device_type: props.device_type,
                    presents,
                }
            })
            .collect()
    });
    unsafe { instance.destroy_instance(None) };
    Ok(devices?)
}

/// The device to use from `devices`, for `wanted`: one of that kind first,
/// then the other kind, then anything else; never one known not to present.
/// None leaves the choice to Blade.
pub(super) fn choose(devices: &[Device], wanted: Kind) -> Option<&Device> {
    let (first, second) = match wanted {
        Kind::Discrete => (
            vk::PhysicalDeviceType::DISCRETE_GPU,
            vk::PhysicalDeviceType::INTEGRATED_GPU,
        ),
        Kind::Integrated => (
            vk::PhysicalDeviceType::INTEGRATED_GPU,
            vk::PhysicalDeviceType::DISCRETE_GPU,
        ),
    };
    let rank = |device: &Device| match device.device_type {
        t if t == first => 0,
        t if t == second => 1,
        vk::PhysicalDeviceType::VIRTUAL_GPU => 2,
        _ => 3,
    };
    let mut usable: Vec<&Device> = devices
        .iter()
        .filter(|device| {
            if device.presents == Some(false) {
                log::info!(
                    target: LOG,
                    "GPU {device} cannot present to this Wayland display; not used"
                );
                false
            } else {
                true
            }
        })
        .collect();
    usable.sort_by_key(|device| rank(device));
    usable.first().copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(name: &str, device_type: vk::PhysicalDeviceType, presents: Option<bool>) -> Device {
        Device {
            name: name.into(),
            vendor_id: 0,
            device_id: name.len() as u32,
            device_type,
            presents,
        }
    }

    fn laptop(nvidia_presents: Option<bool>) -> Vec<Device> {
        vec![
            device("intel", vk::PhysicalDeviceType::INTEGRATED_GPU, Some(true)),
            device(
                "nvidia",
                vk::PhysicalDeviceType::DISCRETE_GPU,
                nvidia_presents,
            ),
            device("llvmpipe", vk::PhysicalDeviceType::CPU, Some(true)),
        ]
    }

    #[test]
    fn the_discrete_gpu_is_preferred_when_there_is_one() {
        let devices = laptop(Some(true));
        assert_eq!(choose(&devices, Kind::Discrete).unwrap().name, "nvidia");
        assert_eq!(choose(&devices, Kind::Integrated).unwrap().name, "intel");
        let devices = laptop(None);
        assert_eq!(choose(&devices, Kind::Discrete).unwrap().name, "nvidia");
    }

    #[test]
    fn a_gpu_that_cannot_present_is_passed_over() {
        let devices = laptop(Some(false));
        assert_eq!(choose(&devices, Kind::Discrete).unwrap().name, "intel");
    }

    #[test]
    fn one_gpu_is_used_whatever_is_asked() {
        let devices = vec![device(
            "intel",
            vk::PhysicalDeviceType::INTEGRATED_GPU,
            None,
        )];
        assert_eq!(choose(&devices, Kind::Discrete).unwrap().name, "intel");
        let devices = vec![device("llvmpipe", vk::PhysicalDeviceType::CPU, None)];
        assert_eq!(choose(&devices, Kind::Integrated).unwrap().name, "llvmpipe");
        assert!(choose(&[], Kind::Discrete).is_none());
    }
}
