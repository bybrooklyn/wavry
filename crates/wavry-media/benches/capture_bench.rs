use criterion::{criterion_group, criterion_main, Criterion};

#[cfg(target_os = "linux")]
use wavry_media::{Codec, EncodeConfig, PipewireEncoder, Resolution};

#[cfg(target_os = "linux")]
fn bench_capture_init(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("pipewire_encoder_init", |b| {
        b.to_async(&rt).iter(|| async {
            // Note: This requires a running portal/PipeWire session
            // In a CI environment, this might fail if not properly set up.
            let config = EncodeConfig {
                codec: Codec::H264,
                resolution: Resolution {
                    width: 1280,
                    height: 720,
                },
                fps: 60,
                bitrate_kbps: 5000,
                keyframe_interval_ms: 2000,
                display_id: None,
                enable_10bit: false,
                enable_hdr: false,
            };
            let _ = PipewireEncoder::new(config).await;
        })
    });
}

#[cfg(not(target_os = "linux"))]
fn bench_capture_init(_c: &mut Criterion) {
    // Stub for non-Linux platforms
}

criterion_group!(benches, bench_capture_init);
criterion_main!(benches);
