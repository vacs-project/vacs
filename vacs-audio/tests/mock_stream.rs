//! Tests that the MockBackend's streams actually invoke callbacks on their
//! background threads. This exercises the play-signal / stop-on-drop
//! handshake which is easy to get wrong with mpsc channels.

#![cfg(feature = "mock-audio")]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use vacs_audio::backend::mock::MockBackend;
use vacs_audio::backend::{AudioBackend, StreamConfig};

#[test]
fn input_stream_invokes_callback() {
    let backend = MockBackend::default();
    let host = backend.default_host();
    let device = host
        .default_input_device()
        .expect("should have input device");

    let call_count = Arc::new(AtomicUsize::new(0));
    let call_count_clone = call_count.clone();

    let config = StreamConfig {
        channels: 1,
        sample_rate: 48000,
        sample_format: cpal::SampleFormat::F32,
        buffer_size: vacs_audio::backend::BufferSize::Default,
    };

    let stream = device
        .build_input_stream_f32(
            &config,
            Box::new(move |data: &[f32]| {
                assert!(!data.is_empty(), "callback should receive non-empty buffer");
                // All samples should be silence (zeros)
                assert!(
                    data.iter().all(|&s| s == 0.0),
                    "mock input should produce silence"
                );
                call_count_clone.fetch_add(1, Ordering::Relaxed);
            }),
            Box::new(|_err| panic!("unexpected stream error")),
        )
        .expect("build_input_stream_f32 should succeed");

    stream.play().expect("play should succeed");

    // Give the mock thread time to tick a few times (20 ms per tick)
    std::thread::sleep(Duration::from_millis(100));

    let count = call_count.load(Ordering::Relaxed);
    assert!(
        count >= 2,
        "expected at least 2 callbacks in 100 ms, got {count}"
    );

    // Drop stream -> stop signal sent -> thread exits cleanly
    drop(stream);
    // Brief sleep to let the thread actually exit
    std::thread::sleep(Duration::from_millis(50));
}

#[test]
fn output_stream_invokes_callback() {
    let backend = MockBackend::default();
    let host = backend.default_host();
    let device = host
        .default_output_device()
        .expect("should have output device");

    let call_count = Arc::new(AtomicUsize::new(0));
    let call_count_clone = call_count.clone();

    let config = StreamConfig {
        channels: 2,
        sample_rate: 48000,
        sample_format: cpal::SampleFormat::F32,
        buffer_size: vacs_audio::backend::BufferSize::Default,
    };

    let stream = device
        .build_output_stream_f32(
            &config,
            Box::new(move |data: &mut [f32]| {
                assert!(!data.is_empty(), "callback should receive non-empty buffer");
                // Write a recognizable pattern (the mock discards it, but
                // this proves the callback can write without panicking)
                for sample in data.iter_mut() {
                    *sample = 0.5;
                }
                call_count_clone.fetch_add(1, Ordering::Relaxed);
            }),
            Box::new(|_err| panic!("unexpected stream error")),
        )
        .expect("build_output_stream_f32 should succeed");

    stream.play().expect("play should succeed");

    std::thread::sleep(Duration::from_millis(100));

    let count = call_count.load(Ordering::Relaxed);
    assert!(
        count >= 2,
        "expected at least 2 callbacks in 100 ms, got {count}"
    );

    drop(stream);
    std::thread::sleep(Duration::from_millis(50));
}

#[test]
fn stream_does_not_callback_before_play() {
    let backend = MockBackend::default();
    let host = backend.default_host();
    let device = host
        .default_input_device()
        .expect("should have input device");

    let call_count = Arc::new(AtomicUsize::new(0));
    let call_count_clone = call_count.clone();

    let config = StreamConfig {
        channels: 1,
        sample_rate: 48000,
        sample_format: cpal::SampleFormat::F32,
        buffer_size: vacs_audio::backend::BufferSize::Default,
    };

    let _stream = device
        .build_input_stream_f32(
            &config,
            Box::new(move |_data: &[f32]| {
                call_count_clone.fetch_add(1, Ordering::Relaxed);
            }),
            Box::new(|_err| panic!("unexpected stream error")),
        )
        .expect("build should succeed");

    // Don't call play() - wait and verify no callbacks fire
    std::thread::sleep(Duration::from_millis(80));

    let count = call_count.load(Ordering::Relaxed);
    assert_eq!(count, 0, "stream should not invoke callbacks before play()");
}
