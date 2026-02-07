use rift_core::Codec as RiftCodec;
#[cfg(target_os = "linux")]
use wavry_media::LinuxProbe;
#[cfg(target_os = "macos")]
use wavry_media::MacProbe;
#[cfg(target_os = "windows")]
use wavry_media::WindowsProbe;
use wavry_media::{CapabilityProbe, Codec};

pub fn local_supported_encoders() -> Vec<Codec> {
    #[cfg(target_os = "windows")]
    {
        return WindowsProbe
            .supported_encoders()
            .unwrap_or_else(|_| vec![Codec::H264]);
    }
    #[cfg(target_os = "macos")]
    {
        return MacProbe
            .supported_encoders()
            .unwrap_or_else(|_| vec![Codec::H264]);
    }
    #[cfg(target_os = "linux")]
    {
        return LinuxProbe
            .supported_encoders()
            .unwrap_or_else(|_| vec![Codec::H264]);
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        vec![Codec::H264]
    }
}

pub fn choose_rift_codec(hello: &rift_core::Hello) -> RiftCodec {
    let local_supported = local_supported_encoders();

    let remote_supported: Vec<RiftCodec> = hello
        .supported_codecs
        .iter()
        .filter_map(|c| RiftCodec::try_from(*c).ok())
        .collect();

    let supports = |codec: RiftCodec| {
        let local_ok = match codec {
            RiftCodec::Av1 => local_supported.contains(&Codec::Av1),
            RiftCodec::Hevc => local_supported.contains(&Codec::Hevc),
            RiftCodec::H264 => local_supported.contains(&Codec::H264),
        };
        let remote_ok = remote_supported.contains(&codec);
        local_ok && remote_ok
    };

    if supports(RiftCodec::Av1) {
        RiftCodec::Av1
    } else if supports(RiftCodec::Hevc) {
        RiftCodec::Hevc
    } else {
        RiftCodec::H264
    }
}
