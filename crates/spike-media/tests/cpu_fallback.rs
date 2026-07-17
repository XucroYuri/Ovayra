use std::{env, path::Path};

use spike_media::{CpuFallback, FfprobeReport};

#[test]
#[ignore = "requires the pinned Phase 0 ffmpeg bundle"]
fn produces_gemini_compatible_vp9_opus_webm() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("fallback.webm");
    let ffmpeg = env::var("OVAYRA_FFMPEG").expect("OVAYRA_FFMPEG must point to the pinned bundle");
    let ffprobe =
        env::var("OVAYRA_FFPROBE").expect("OVAYRA_FFPROBE must point to the pinned bundle");
    CpuFallback::new(ffmpeg, &ffprobe)
        .generate_synthetic(&output, 3)
        .unwrap();
    let report = FfprobeReport::read(ffprobe, &output).unwrap();
    assert_eq!(report.container, "matroska,webm");
    assert_eq!(report.video_codec.as_deref(), Some("vp9"));
    assert_eq!(report.audio_codec.as_deref(), Some("opus"));
    assert_eq!(report.video_pixel_format.as_deref(), Some("yuv420p"));
}

#[test]
fn ffmpeg_arguments_are_canonical_and_honor_the_requested_duration() {
    let arguments = CpuFallback::new("ffmpeg", "ffprobe")
        .ffmpeg_arguments(Path::new("target/phase-0/fallback.webm"), 10);
    let arguments = arguments
        .iter()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        arguments,
        vec![
            "-y",
            "-hide_banner",
            "-nostdin",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=640x360:rate=24",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=1000:sample_rate=48000",
            "-t",
            "10",
            "-map",
            "0:v:0",
            "-map",
            "1:a:0",
            "-c:v",
            "libvpx-vp9",
            "-deadline",
            "realtime",
            "-cpu-used",
            "4",
            "-b:v",
            "600k",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "libopus",
            "-b:a",
            "64k",
            "-ac",
            "1",
            "-f",
            "webm",
            "-progress",
            "pipe:1",
            "-nostats",
            "target/phase-0/fallback.webm",
        ]
    );
    assert!(
        CpuFallback::new("ffmpeg", "ffprobe")
            .ffmpeg_arguments(Path::new("out.webm"), 3)
            .windows(2)
            .any(|pair| pair[0] == "-t" && pair[1] == "3")
    );
}

#[test]
fn ffprobe_json_rejects_invalid_media_reports() {
    assert!(FfprobeReport::from_child_output(false, b"{}", 12).is_err());
    for input in [
        "not json",
        r#"{"format":{"format_name":"mp4","duration":"2"},"streams":[]}"#,
        r#"{"format":{"format_name":"matroska,webm","duration":"2"},"streams":[]}"#,
        r#"{"format":{"format_name":"matroska,webm","duration":"2"},"streams":[{"codec_type":"video","codec_name":"h264","pix_fmt":"yuv420p"},{"codec_type":"audio","codec_name":"opus"}]}"#,
        r#"{"format":{"format_name":"matroska,webm","duration":"0"},"streams":[{"codec_type":"video","codec_name":"vp9","pix_fmt":"yuv420p"},{"codec_type":"audio","codec_name":"opus"}]}"#,
    ] {
        assert!(FfprobeReport::from_json(input, 12).is_err(), "{input}");
    }
    assert!(FfprobeReport::from_json(
        r#"{"format":{"format_name":"matroska,webm","duration":"2"},"streams":[{"codec_type":"video","codec_name":"vp9","pix_fmt":"yuv420p"},{"codec_type":"audio","codec_name":"opus"}]}"#,
        0,
    )
    .is_err());
}

#[test]
fn average_speed_uses_progress_events_and_evidence_values_are_redacted() {
    let speed = CpuFallback::average_speed_from_progress(
        b"speed=1.0x\nprogress=continue\nspeed=3.0x\nprogress=end\n",
    )
    .unwrap();
    assert_eq!(speed, Some(2.0));
    assert_eq!(
        spike_media::content_sha256_bytes(b"synthetic media"),
        "690758e29609deeb256114e268bd1ede8c4e77eb2406044e4943433a52a8598d"
    );
    assert!(spike_media::redacted_process_detail("/private/path\nstderr detail").is_none());
}
