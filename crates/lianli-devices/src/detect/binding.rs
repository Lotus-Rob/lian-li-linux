use super::backends::usb_topology_string;
use super::enumerate::enumerate_devices;
use lianli_transport::RusbHid;
use lianli_transport::UsbContext;
use rusb::Device;
use std::path::Path;
use std::time::Duration;
use tracing::{info, warn};

const USB_SYSFS: &str = "/sys/bus/usb/devices";

/// Some devices come back from suspend or reboot with a malformed HID report
/// descriptor, which the kernel rejects, leaving no hidraw node. A USB port reset
/// makes the firmware serve a valid one again.
pub fn ensure_hid_devices_bound() {
    let devices = match enumerate_devices() {
        Ok(devices) => devices,
        Err(e) => {
            warn!("Skipping HID binding check, USB enumeration failed: {e}");
            return;
        }
    };

    let mut reset_count = 0u32;
    for det in devices.iter().filter(|d| d.family.uses_hid()) {
        let ports = match det.device.port_numbers() {
            Ok(ports) if !ports.is_empty() => ports,
            _ => continue,
        };
        let topology = usb_topology_string(det.bus, &ports);
        let sysfs = Path::new(USB_SYSFS).join(&topology);
        if !lacks_hid_node(&sysfs) {
            continue;
        }
        let label = format!(
            "{} ({:04x}:{:04x}) at {topology}",
            det.name, det.vid, det.pid
        );
        info!("{label}: HID interface has no hidraw node");
        if device_responds_to_descriptor(&det.device) {
            nudge_kernel_to_bind_hid(&det.device);
            if wait_until_bound(&sysfs, Duration::from_secs(2)) {
                info!("{label}: hidraw appeared after kernel bind, no reset needed");
                continue;
            }
        }
        info!("{label}: performing USB reset");
        match RusbHid::reset_usb_device(&det.device) {
            Ok(()) => reset_count += 1,
            // A reset that re-enumerates the device invalidates the handle it was issued on.
            Err(e) if e.to_string().contains("Entity not found") => reset_count += 1,
            Err(e) => warn!("{label}: USB reset failed: {e}"),
        }
    }

    if reset_count > 0 {
        info!("Waiting 3s for {reset_count} device(s) to re-enumerate after USB reset");
        std::thread::sleep(Duration::from_secs(3));
    }
}

/// A device counts as bound when any HID interface has a node, because some
/// multi-interface devices may leave secondary interfaces without one. Unreadable
/// sysfs entries also count as bound, so missing visibility, such as inside a
/// container, never triggers a reset.
fn lacks_hid_node(device_dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(device_dir) else {
        return false;
    };
    let mut interfaces = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| is_hid_interface(path))
        .peekable();
    interfaces.peek().is_some() && !interfaces.any(|interface| has_kernel_hid_node(&interface))
}

fn is_hid_interface(path: &Path) -> bool {
    std::fs::read_to_string(path.join("bInterfaceClass")).is_ok_and(|class| class.trim() == "03")
}

fn has_kernel_hid_node(interface: &Path) -> bool {
    let Ok(children) = std::fs::read_dir(interface) else {
        return true;
    };
    children
        .flatten()
        .map(|child| child.path())
        .any(|hid| hid.join("hidraw").is_dir() || hid.join("hwmon").is_dir())
}

fn wait_until_bound(device_dir: &Path, max_wait: Duration) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < max_wait {
        if !lacks_hid_node(device_dir) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

fn nudge_kernel_to_bind_hid(device: &Device<UsbContext>) {
    let Ok(handle) = device.open() else {
        return;
    };
    let Ok(config) = device.active_config_descriptor() else {
        return;
    };
    for iface in config.interfaces() {
        for desc in iface.descriptors() {
            if desc.class_code() == 0x03 {
                let _ = handle.attach_kernel_driver(desc.interface_number());
            }
        }
    }
}

fn device_responds_to_descriptor(device: &Device<UsbContext>) -> bool {
    let Ok(desc) = device.device_descriptor() else {
        return false;
    };
    let Ok(handle) = device.open() else {
        return false;
    };
    let langs = match handle.read_languages(Duration::from_millis(250)) {
        Ok(l) if !l.is_empty() => l,
        _ => return desc.product_string_index().is_some(),
    };
    if let Some(_idx) = desc.product_string_index() {
        return handle
            .read_product_string(langs[0], &desc, Duration::from_millis(250))
            .is_ok();
    }
    true
}

#[cfg(test)]
mod tests {
    use super::lacks_hid_node;
    use std::fs;
    use std::path::{Path, PathBuf};

    struct Fixture(PathBuf);

    impl Fixture {
        fn new(name: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("lianli-binding-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }

        fn interface(&self, name: &str, class: &str, node: Option<&str>) {
            let interface = self.0.join(name);
            fs::create_dir_all(&interface).unwrap();
            fs::write(interface.join("bInterfaceClass"), format!("{class}\n")).unwrap();
            if let Some(node) = node {
                fs::create_dir_all(interface.join("0003:0416:7395.0011").join(node)).unwrap();
            }
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn hid_interface_without_hidraw_lacks_a_node() {
        let device = Fixture::new("unbound");
        device.interface("1-13.3:1.0", "ff", None);
        device.interface("1-13.3:1.1", "03", None);
        assert!(lacks_hid_node(device.path()));
    }

    #[test]
    fn hwmon_node_counts_as_bound() {
        let device = Fixture::new("hwmon");
        device.interface("1-13.3:1.0", "03", Some("hwmon"));
        assert!(!lacks_hid_node(device.path()));
    }

    #[test]
    fn one_bound_interface_is_enough_for_multi_interface_devices() {
        let device = Fixture::new("multi");
        device.interface("1-4:1.0", "03", Some("hidraw"));
        device.interface("1-4:1.1", "03", None);
        assert!(!lacks_hid_node(device.path()));
    }

    #[test]
    fn device_without_hid_interfaces_never_lacks_a_node() {
        let device = Fixture::new("vendor-only");
        device.interface("1-4:1.0", "ff", None);
        assert!(!lacks_hid_node(device.path()));
    }

    #[test]
    fn invisible_device_never_lacks_a_node() {
        let device = Fixture::new("invisible");
        assert!(!lacks_hid_node(&device.path().join("1-9")));
    }
}
