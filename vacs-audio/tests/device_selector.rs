//! Integration tests for audio backend device selection using the MockBackend.
//!
//! These tests exercise the config scoring logic, device selection priority
//! (ID -> exact name -> substring -> default), and fallback behavior - all
//! without touching real audio hardware.

#![cfg(feature = "mock-audio")]

use vacs_audio::backend::mock::{MockBackend, MockBackendConfig, MockDeviceConfig};
use vacs_audio::device::{AudioBackendExt, DeviceType};

fn two_input_backend() -> MockBackend {
    MockBackend::new(MockBackendConfig {
        host_name: "TestHost".to_string(),
        input_devices: vec![
            MockDeviceConfig {
                name: "USB Mic".to_string(),
                id: "usb-mic-0".to_string(),
                min_sample_rate: 44100,
                max_sample_rate: 44100,
                channels: 1,
            },
            MockDeviceConfig {
                name: "Studio Condenser".to_string(),
                id: "studio-1".to_string(),
                min_sample_rate: 48000,
                max_sample_rate: 48000,
                channels: 1,
            },
        ],
        output_devices: vec![MockDeviceConfig {
            name: "Headphones".to_string(),
            id: "hp-0".to_string(),
            min_sample_rate: 48000,
            max_sample_rate: 48000,
            channels: 2,
        }],
    })
}

// ── Config scoring ──────────────────────────────────────────────────────

#[test]
fn picks_default_over_highest_score() {
    // The scoring logic should rank 48 kHz (TARGET_SAMPLE_RATE) above 44.1 kHz
    // because it avoids resampling.
    let backend = two_input_backend();

    let (device, _) = backend
        .open(DeviceType::Input, None, None, None)
        .expect("open should succeed");

    // The default device is the first one (USB Mic @ 44.1 kHz) but
    // pick_best_stream_config should still succeed. However, without a
    // preferred device, DeviceSelector picks the *default* (first) device,
    // not the best-scored one - the scoring only selects the best *config*
    // for a given device. So default -> USB Mic.
    //
    // This is intentional: the user's default device is respected even if
    // another device has a "better" sample rate.
    assert_eq!(device.name(), "USB Mic");
    assert_eq!(device.sample_rate(), 44100);
}

#[test]
fn picks_48khz_when_range_includes_target() {
    // Device supports 16 kHz – 96 kHz -> scoring should pick exactly 48 kHz
    // from within the range, even though neither endpoint matches.
    let backend = MockBackend::new(MockBackendConfig {
        host_name: "H".to_string(),
        input_devices: vec![MockDeviceConfig {
            name: "Wide Range Mic".to_string(),
            id: "wr-0".to_string(),
            min_sample_rate: 16000,
            max_sample_rate: 96000,
            channels: 1,
        }],
        output_devices: vec![MockDeviceConfig {
            name: "Speaker".to_string(),
            id: "sp-0".to_string(),
            min_sample_rate: 48000,
            max_sample_rate: 48000,
            channels: 2,
        }],
    });

    let (device, _) = backend
        .open(DeviceType::Input, None, None, None)
        .expect("open should succeed");

    assert_eq!(device.sample_rate(), 48000);
}

// ── Device selection priority ───────────────────────────────────────────

#[test]
fn selects_device_by_id() {
    let backend = two_input_backend();

    let (device, is_fallback) = backend
        .open(DeviceType::Input, None, Some("studio-1"), None)
        .expect("open should succeed");

    assert_eq!(device.name(), "Studio Condenser");
    assert!(!is_fallback);
}

#[test]
fn selects_device_by_exact_name() {
    let backend = two_input_backend();

    let (device, is_fallback) = backend
        .open(DeviceType::Input, None, None, Some("Studio Condenser"))
        .expect("open should succeed");

    assert_eq!(device.name(), "Studio Condenser");
    assert!(!is_fallback);
}

#[test]
fn selects_device_by_case_insensitive_name() {
    let backend = two_input_backend();

    let (device, is_fallback) = backend
        .open(DeviceType::Input, None, None, Some("studio condenser"))
        .expect("open should succeed");

    assert_eq!(device.name(), "Studio Condenser");
    assert!(!is_fallback);
}

#[test]
fn selects_device_by_substring_name() {
    let backend = two_input_backend();

    let (device, is_fallback) = backend
        .open(DeviceType::Input, None, None, Some("Condenser"))
        .expect("open should succeed");

    assert_eq!(device.name(), "Studio Condenser");
    assert!(!is_fallback);
}

#[test]
fn id_takes_priority_over_name() {
    // When both ID and name are provided, the ID should win.
    let backend = two_input_backend();

    let (device, is_fallback) = backend
        .open(
            DeviceType::Input,
            None,
            Some("usb-mic-0"),        // ID -> USB Mic
            Some("Studio Condenser"), // Name -> Studio Condenser
        )
        .expect("open should succeed");

    assert_eq!(device.name(), "USB Mic");
    assert!(!is_fallback);
}

#[test]
fn falls_back_to_default_when_preferred_not_found() {
    let backend = two_input_backend();

    let (device, is_fallback) = backend
        .open(
            DeviceType::Input,
            None,
            Some("nonexistent-id"),
            Some("Nonexistent Device"),
        )
        .expect("open should succeed");

    // Should fall back to the default (first) device, and flag it as fallback.
    assert_eq!(device.name(), "USB Mic");
    assert!(is_fallback);
}

#[test]
fn resolve_device_id_by_name() {
    let backend = two_input_backend();

    let id = backend.resolve_device_id(DeviceType::Input, None, "Studio Condenser");
    assert_eq!(id, Some("studio-1".to_string()));
}

#[test]
fn resolve_device_id_returns_none_for_unknown() {
    let backend = two_input_backend();

    let id = backend.resolve_device_id(DeviceType::Input, None, "Ghost Mic");
    assert_eq!(id, None);
}

// ── Host selection ──────────────────────────────────────────────────────

#[test]
fn selects_host_by_name_case_insensitive() {
    let backend = two_input_backend();

    // "testhost" should match "TestHost"
    let (device, _) = backend
        .open(DeviceType::Output, Some("testhost"), None, None)
        .expect("should match host case-insensitively");

    assert_eq!(device.name(), "Headphones");
}

#[test]
fn all_host_and_device_names() {
    let backend = two_input_backend();

    let hosts = backend.all_host_names();
    assert_eq!(hosts, vec!["TestHost"]);

    let inputs = backend.all_device_names(DeviceType::Input, None).unwrap();
    assert_eq!(inputs.len(), 2);
    assert!(inputs.contains(&"USB Mic".to_string()));
    assert!(inputs.contains(&"Studio Condenser".to_string()));

    let outputs = backend.all_device_names(DeviceType::Output, None).unwrap();
    assert_eq!(outputs, vec!["Headphones"]);
}

// ── Output device scoring ───────────────────────────────────────────────

#[test]
fn output_defaults_to_first_device_regardless_of_channels() {
    // The default (first) output device is picked even when a stereo
    // alternative exists. Explicit selection by ID still works.
    let backend = MockBackend::new(MockBackendConfig {
        host_name: "H".to_string(),
        input_devices: vec![MockDeviceConfig {
            name: "Mic".to_string(),
            id: "m".to_string(),
            min_sample_rate: 48000,
            max_sample_rate: 48000,
            channels: 1,
        }],
        output_devices: vec![
            MockDeviceConfig {
                name: "Mono Speaker".to_string(),
                id: "mono".to_string(),
                min_sample_rate: 48000,
                max_sample_rate: 48000,
                channels: 1,
            },
            MockDeviceConfig {
                name: "Stereo Speaker".to_string(),
                id: "stereo".to_string(),
                min_sample_rate: 48000,
                max_sample_rate: 48000,
                channels: 2,
            },
        ],
    });

    // Default is first device (Mono Speaker).
    let (device, _) = backend.open(DeviceType::Output, None, None, None).unwrap();
    assert_eq!(device.name(), "Mono Speaker");
    assert_eq!(device.channels(), 1);

    // Explicitly selecting the stereo device works fine.
    let (device, _) = backend
        .open(DeviceType::Output, None, Some("stereo"), None)
        .unwrap();
    assert_eq!(device.name(), "Stereo Speaker");
    assert_eq!(device.channels(), 2);
}
