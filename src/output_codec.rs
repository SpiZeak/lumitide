//! Detect the transport codec of the current output device (e.g. LDAC over
//! Bluetooth) so the UI can show it next to the stream quality label.
//!
//! Linux only: queries the PipeWire/PulseAudio `pactl` utility. Any failure
//! (missing binary, wired output, unknown codec) yields `None`, in which case
//! no transport badge is shown.

/// Bluetooth transport codec of the default output sink, display-formatted
/// (e.g. "LDAC"). `None` when the default sink is not Bluetooth or the audio
/// server cannot be queried.
pub fn detect() -> Option<String> {
    detect_impl()
}

#[cfg(target_os = "linux")]
fn detect_impl() -> Option<String> {
    let default_sink = run_pactl(&["get-default-sink"])?;
    let list = run_pactl(&["list", "sinks"])?;
    let raw = parse_codec(&default_sink, &list)?;
    Some(display_name(&raw))
}

#[cfg(not(target_os = "linux"))]
fn detect_impl() -> Option<String> {
    None
}

#[cfg(target_os = "linux")]
fn run_pactl(args: &[&str]) -> Option<String> {
    std::process::Command::new("pactl")
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
}

/// Extract `api.bluez5.codec` from the `pactl list sinks` block whose
/// `Name:` matches `default_sink`.
fn parse_codec(default_sink: &str, list_output: &str) -> Option<String> {
    let target = default_sink.trim();
    let mut in_target = false;
    for line in list_output.lines() {
        if line.starts_with("Sink #") {
            in_target = false;
        } else if let Some(name) = line.trim_start().strip_prefix("Name: ") {
            in_target = name.trim() == target;
        } else if in_target {
            if let Some(rest) = line.trim_start().strip_prefix("api.bluez5.codec = ") {
                return Some(rest.trim().trim_matches('"').to_string());
            }
        }
    }
    None
}

/// Map PipeWire's raw codec names to their conventional display names.
fn display_name(raw: &str) -> String {
    match raw {
        "ldac" => "LDAC".to_string(),
        "sbc" => "SBC".to_string(),
        "sbc_xq" => "SBC-XQ".to_string(),
        "aac" => "AAC".to_string(),
        "mp3" => "MP3".to_string(),
        "aptx" => "aptX".to_string(),
        "aptx_hd" => "aptX HD".to_string(),
        "aptx_ll" => "aptX Low Latency".to_string(),
        other if other.starts_with("aptx_adaptive") => "aptX Adaptive".to_string(),
        other if other.starts_with("lc3") => "LC3".to_string(),
        other => other.to_uppercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = "\
Sink #57
\tState: SUSPENDED
\tName: alsa_output.pci-0000_00_05_0.analog-stereo
\tDescription: Ryzen HD Audio Controller Analog Stereo
\tProperties:
\t\tdevice.description = \"Ryzen HD Audio Controller Analog Stereo\"
Sink #92
\tState: RUNNING
\tName: bluez_output.F4_9D_8A_C0_8E_80.1
\tDescription: soundcore Space One
\tProperties:
\t\tapi.bluez5.codec = \"ldac\"
\t\tdevice.alias = \"soundcore Space One\"
";

    #[test]
    fn parses_codec_of_default_bluetooth_sink() {
        let codec = parse_codec("bluez_output.F4_9D_8A_C0_8E_80.1", LIST);
        assert_eq!(codec.as_deref(), Some("ldac"));
    }

    #[test]
    fn wired_sink_has_no_codec() {
        assert_eq!(parse_codec("alsa_output.pci-0000_00_05_0.analog-stereo", LIST), None);
    }

    #[test]
    fn unknown_sink_has_no_codec() {
        assert_eq!(parse_codec("alsa_output.does-not-exist", LIST), None);
    }

    #[test]
    fn codec_names_are_display_formatted() {
        assert_eq!(display_name("ldac"), "LDAC");
        assert_eq!(display_name("sbc_xq"), "SBC-XQ");
        assert_eq!(display_name("aptx_hd"), "aptX HD");
        assert_eq!(display_name("aptx_adaptive_II"), "aptX Adaptive");
    }
}
