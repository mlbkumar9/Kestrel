use crate::encoders::EncoderSet;
use crate::formats::Container;
use crate::transcode::{
    AudioChoice, AudioSettings, BurnIn, HwCodec, JobSettings, Mixdown, RateControl,
    SubtitleSettings, TenBit, VideoEncode, VideoEncoder, VideoSettings,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub description: String,
    pub builtin: bool,
    pub settings: JobSettings,
}

#[derive(Deserialize, Serialize)]
struct PresetExport {
    version: u32,
    presets: Vec<Preset>,
}

pub fn builtin_preset(id: &str) -> Option<Preset> {
    builtins_without_hardware()
        .into_iter()
        .find(|preset| preset.id == id)
}

pub fn builtins(encoders: &EncoderSet) -> Vec<Preset> {
    let mut presets = builtins_without_hardware();
    if let Some(encoder) = encoders.hardware.iter().find(|encoder| {
        matches!(
            encoder,
            VideoEncoder::Hardware {
                codec: HwCodec::Hevc,
                ..
            }
        )
    }) {
        presets.push(Preset {
            id: "hardware".to_owned(),
            name: "Fast hardware encode".to_owned(),
            description: "Encode HEVC using the first available hardware encoder.".to_owned(),
            builtin: true,
            settings: JobSettings {
                container: Container::Mkv,
                video: VideoSettings::Encode(VideoEncode {
                    encoder: encoder.clone(),
                    rate: RateControl::Quality(60),
                    speed: 3,
                    ..VideoEncode::default()
                }),
                ..JobSettings::default()
            },
        });
    }
    presets
}

pub fn rename_user_preset(
    presets: &mut [Preset],
    id: &str,
    name: impl Into<String>,
) -> Result<(), String> {
    let preset = presets
        .iter_mut()
        .find(|preset| preset.id == id && !preset.builtin)
        .ok_or_else(|| "User preset was not found".to_owned())?;
    let name = name.into();
    if name.trim().is_empty() {
        return Err("Preset name cannot be empty".to_owned());
    }
    preset.name = name.trim().to_owned();
    Ok(())
}

pub fn delete_user_preset(presets: &mut Vec<Preset>, id: &str) -> bool {
    let initial_len = presets.len();
    presets.retain(|preset| preset.id != id || preset.builtin);
    presets.len() != initial_len
}

pub fn export_user_presets(presets: &[Preset]) -> Result<String, String> {
    serde_json::to_string_pretty(&PresetExport {
        version: 1,
        presets: presets
            .iter()
            .filter(|preset| !preset.builtin)
            .cloned()
            .collect(),
    })
    .map_err(|error| format!("Could not export presets: {error}"))
}

pub fn import_user_presets(json: &str) -> Result<Vec<Preset>, String> {
    let mut exported: PresetExport =
        serde_json::from_str(json).map_err(|error| format!("Invalid preset file: {error}"))?;
    if exported.version != 1 {
        return Err(format!(
            "Unsupported preset file version: {}",
            exported.version
        ));
    }
    for preset in &mut exported.presets {
        preset.builtin = false;
    }
    Ok(exported.presets)
}

fn builtins_without_hardware() -> Vec<Preset> {
    vec![
        Preset {
            id: "remux".to_owned(),
            name: "Change tracks only".to_owned(),
            description: "Copy streams and select tracks without re-encoding.".to_owned(),
            builtin: true,
            settings: JobSettings::default(),
        },
        Preset {
            id: "smaller".to_owned(),
            name: "Same quality, smaller file".to_owned(),
            description: "Encode video with HEVC and preserve audio where compatible.".to_owned(),
            builtin: true,
            settings: JobSettings {
                container: Container::Mkv,
                video: VideoSettings::Encode(VideoEncode {
                    encoder: VideoEncoder::X265,
                    rate: RateControl::Quality(60),
                    speed: 2,
                    ..VideoEncode::default()
                }),
                ..JobSettings::default()
            },
        },
        Preset {
            id: "anywhere".to_owned(),
            name: "Play anywhere".to_owned(),
            description: "Create an MP4 with broadly compatible video and audio.".to_owned(),
            builtin: true,
            settings: JobSettings {
                container: Container::Mp4,
                video: VideoSettings::Encode(VideoEncode {
                    encoder: VideoEncoder::X264,
                    rate: RateControl::Quality(60),
                    speed: 2,
                    max_height: Some(1080),
                    ..VideoEncode::default()
                }),
                audio: AudioSettings {
                    codec: AudioChoice::Aac,
                    bitrate_kbps: 160,
                    mixdown: Mixdown::Stereo,
                    ..AudioSettings::default()
                },
                subtitles: SubtitleSettings {
                    burn_in: BurnIn::ForcedOnly,
                    ..SubtitleSettings::default()
                },
                ..JobSettings::default()
            },
        },
        Preset {
            id: "archive".to_owned(),
            name: "Archive (AV1)".to_owned(),
            description: "Encode a high-efficiency ten-bit AV1 archive.".to_owned(),
            builtin: true,
            settings: JobSettings {
                container: Container::Mkv,
                video: VideoSettings::Encode(VideoEncode {
                    encoder: VideoEncoder::SvtAv1,
                    rate: RateControl::Quality(65),
                    speed: 1,
                    ten_bit: TenBit::On,
                    ..VideoEncode::default()
                }),
                ..JobSettings::default()
            },
        },
        Preset {
            id: "web".to_owned(),
            name: "Web (WebM)".to_owned(),
            description: "Create a WebM with VP9 video and Opus audio.".to_owned(),
            builtin: true,
            settings: JobSettings {
                container: Container::WebM,
                video: VideoSettings::Encode(VideoEncode {
                    encoder: VideoEncoder::Vp9,
                    rate: RateControl::Quality(55),
                    speed: 2,
                    max_height: Some(1080),
                    ..VideoEncode::default()
                }),
                audio: AudioSettings {
                    codec: AudioChoice::Opus,
                    bitrate_kbps: 128,
                    mixdown: Mixdown::Stereo,
                    ..AudioSettings::default()
                },
                ..JobSettings::default()
            },
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hardware_encoder() -> VideoEncoder {
        VideoEncoder::Hardware {
            vendor: crate::transcode::HwVendor::Nvenc,
            codec: HwCodec::Hevc,
        }
    }

    #[test]
    fn software_presets_have_stable_settings() {
        let presets = builtins(&EncoderSet::default());
        assert_eq!(
            presets
                .iter()
                .map(|preset| preset.id.as_str())
                .collect::<Vec<_>>(),
            ["remux", "smaller", "anywhere", "archive", "web"]
        );
        assert!(presets[0].settings.is_remux_only());
        assert_eq!(presets[1].settings.container, Container::Mkv);
        assert_eq!(
            presets[1].settings.video,
            VideoSettings::Encode(VideoEncode {
                encoder: VideoEncoder::X265,
                rate: RateControl::Quality(60),
                speed: 2,
                ..VideoEncode::default()
            })
        );
        assert_eq!(presets[2].settings.container, Container::Mp4);
        assert_eq!(presets[2].settings.subtitles.burn_in, BurnIn::ForcedOnly);
        assert_eq!(
            presets[3].settings.video,
            VideoSettings::Encode(VideoEncode {
                encoder: VideoEncoder::SvtAv1,
                rate: RateControl::Quality(65),
                speed: 1,
                ten_bit: TenBit::On,
                ..VideoEncode::default()
            })
        );
        assert_eq!(presets[4].settings.container, Container::WebM);
        assert_eq!(presets[4].settings.audio.codec, AudioChoice::Opus);
    }

    #[test]
    fn hardware_preset_is_only_listed_when_hevc_is_available() {
        assert!(
            builtins(&EncoderSet::default())
                .iter()
                .all(|preset| preset.id != "hardware")
        );
        let encoders = EncoderSet {
            software: Vec::new(),
            hardware: vec![hardware_encoder()],
        };
        let preset = builtins(&encoders).pop().unwrap();
        assert_eq!(preset.id, "hardware");
        assert_eq!(
            match preset.settings.video {
                VideoSettings::Encode(encode) => encode.encoder,
                VideoSettings::Copy => panic!("hardware preset must encode"),
            },
            hardware_encoder()
        );
    }

    #[test]
    fn imports_exports_and_manages_user_presets() {
        let mut user = Preset {
            id: "custom-1".to_owned(),
            name: "Custom".to_owned(),
            description: String::new(),
            builtin: false,
            settings: JobSettings::default(),
        };
        let json = export_user_presets(std::slice::from_ref(&user)).unwrap();
        let imported = import_user_presets(&json).unwrap();
        assert_eq!(imported, [user.clone()]);
        rename_user_preset(std::slice::from_mut(&mut user), "custom-1", "Renamed").unwrap();
        assert_eq!(user.name, "Renamed");
        assert!(delete_user_preset(&mut vec![user], "custom-1"));
        assert!(import_user_presets(r#"{"version":2,"presets":[]}"#).is_err());
    }
}
