use crate::{CapabilityProbe, Codec, DisplayInfo, Resolution};
use anyhow::Result;

pub struct AndroidProbe;

impl CapabilityProbe for AndroidProbe {
    fn supported_encoders(&self) -> Result<Vec<Codec>> {
        // Android typically supports H264 and HEVC via MediaCodec
        Ok(vec![Codec::H264, Codec::Hevc])
    }

    fn supported_decoders(&self) -> Result<Vec<Codec>> {
        Ok(vec![Codec::H264, Codec::Hevc, Codec::Av1])
    }

    fn enumerate_displays(&self) -> Result<Vec<DisplayInfo>> {
        Ok(vec![DisplayInfo {
            id: 0,
            name: "Internal Display".to_string(),
            resolution: Resolution {
                width: 1080,
                height: 1920,
            },
        }])
    }
}
