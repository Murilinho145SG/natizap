//! The microphones, speakers and cameras a call can use, and which of them
//! it does. Listing them probes the system, which can take a while: do it
//! off the UI thread.

use cpal::traits::{DeviceTrait as _, HostTrait as _};

/// A device to choose: what stays the same across runs, to keep the choice
/// by, and the name it shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// What identifies the device across runs.
    pub key: String,
    /// What the device is called.
    pub name: String,
}

/// The devices a call uses; `None` is the system's default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CallDevices {
    /// The microphone, by its [`Device::key`].
    pub microphone: Option<String>,
    /// The speaker or headset, by its [`Device::key`].
    pub speaker: Option<String>,
    /// The camera, by its [`Device::key`].
    pub camera: Option<String>,
}

/// The microphones there are now.
#[must_use]
pub fn microphones() -> Vec<Device> {
    cpal::default_host().input_devices().map_or_else(
        |_| Vec::new(),
        |devices| {
            devices
                .filter_map(|device| described(&device))
                .filter(|device| !is_monitor(&device.key))
                .collect()
        },
    )
}

/// Whether the input `key` names is a sound server's monitor: what a
/// speaker plays, offered as an input, which hears nobody.
fn is_monitor(key: &str) -> bool {
    // PulseAudio, and PipeWire speaking its protocol, name a sink's monitor
    // source after the sink with this ending.
    key.ends_with(".monitor")
}

/// The speakers and headsets there are now.
#[must_use]
pub fn speakers() -> Vec<Device> {
    cpal::default_host().output_devices().map_or_else(
        |_| Vec::new(),
        |devices| devices.filter_map(|device| described(&device)).collect(),
    )
}

/// The cameras there are now.
#[must_use]
pub fn cameras() -> Vec<Device> {
    if cfg!(target_os = "linux") {
        v4l2_cameras()
    } else {
        // FFmpeg's inputs on the other systems open the first camera by
        // number; telling them apart needs the system's own listing.
        vec![Device {
            key: default_camera().to_owned(),
            name: "Camera".to_owned(),
        }]
    }
}

/// The device FFmpeg opens when no camera was chosen.
pub(crate) fn default_camera() -> &'static str {
    if cfg!(target_os = "macos") {
        "0"
    } else if cfg!(target_os = "windows") {
        "video=0"
    } else {
        "/dev/video0"
    }
}

/// The microphone `key` names, else the default one.
pub(crate) fn microphone(key: Option<&str>) -> Option<cpal::Device> {
    let host = cpal::default_host();
    key.and_then(|key| {
        host.input_devices()
            .ok()?
            .find(|device| id(device).as_deref() == Some(key))
    })
    .or_else(|| host.default_input_device())
}

/// The speaker `key` names, else the default one.
pub(crate) fn speaker(key: Option<&str>) -> Option<cpal::Device> {
    let host = cpal::default_host();
    key.and_then(|key| {
        host.output_devices()
            .ok()?
            .find(|device| id(device).as_deref() == Some(key))
    })
    .or_else(|| host.default_output_device())
}

fn id(device: &cpal::Device) -> Option<String> {
    device.id().ok().map(|id| id.to_string())
}

fn described(device: &cpal::Device) -> Option<Device> {
    Some(Device {
        key: id(device)?,
        name: device.description().ok()?.name().to_owned(),
    })
}

/// The video devices that capture pictures: a camera also shows up as a
/// node for its metadata, which the kernel numbers after its first.
fn v4l2_cameras() -> Vec<Device> {
    let Ok(nodes) = std::fs::read_dir("/sys/class/video4linux") else {
        return Vec::new();
    };
    let mut cameras: Vec<Device> = nodes
        .filter_map(Result::ok)
        .filter(|node| read(&node.path().join("index")).as_deref() == Some("0"))
        .filter_map(|node| {
            let name = read(&node.path().join("name"))?;
            let file = node.file_name().into_string().ok()?;
            Some(Device {
                key: format!("/dev/{file}"),
                name,
            })
        })
        .collect();
    cameras.sort_by(|a, b| a.key.cmp(&b.key));
    cameras
}

fn read(path: &std::path::Path) -> Option<String> {
    Some(std::fs::read_to_string(path).ok()?.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::is_monitor;

    #[test]
    fn a_speakers_monitor_is_no_microphone() {
        assert!(is_monitor(
            "pulseaudio:alsa_output.pci-0000_10_00.6.analog-stereo.monitor"
        ));
        assert!(!is_monitor(
            "pulseaudio:alsa_input.usb-fifine_Microphone-00.mono-fallback"
        ));
    }
}
