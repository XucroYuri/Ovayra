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
fn fixture_ffprobe_requires_h264_aac_and_a_positive_duration() {
    let valid = r#"{"format":{"format_name":"mov,mp4,m4a,3gp,3g2,mj2","duration":"10.0"},"streams":[{"codec_type":"video","codec_name":"h264","pix_fmt":"yuv420p"},{"codec_type":"audio","codec_name":"aac"}]}"#;
    let report = FfprobeReport::from_h264_aac_json(valid, 12).unwrap();
    assert_eq!(report.video_codec.as_deref(), Some("h264"));
    assert_eq!(report.audio_codec.as_deref(), Some("aac"));
    assert!((report.duration_seconds - 10.0).abs() < f64::EPSILON);

    for invalid in [
        r#"{"format":{"format_name":"mp4","duration":"10"},"streams":[{"codec_type":"video","codec_name":"vp9"},{"codec_type":"audio","codec_name":"aac"}]}"#,
        r#"{"format":{"format_name":"mp4","duration":"10"},"streams":[{"codec_type":"video","codec_name":"h264"},{"codec_type":"audio","codec_name":"opus"}]}"#,
        r#"{"format":{"format_name":"mp4","duration":"0"},"streams":[{"codec_type":"video","codec_name":"h264"},{"codec_type":"audio","codec_name":"aac"}]}"#,
    ] {
        assert!(FfprobeReport::from_h264_aac_json(invalid, 12).is_err());
    }
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

#[test]
fn rejects_a_zero_requested_duration_before_spawning() {
    let error = CpuFallback::new("not-a-real-ffmpeg", "not-a-real-ffprobe")
        .generate_synthetic(Path::new("unused.webm"), 0)
        .unwrap_err();
    assert!(matches!(
        error,
        spike_media::CpuFallbackError::InvalidRequestedDuration
    ));
}

#[cfg(unix)]
mod child_processes {
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
        process::Stdio,
        time::{Duration, Instant},
    };

    use spike_media::{CpuFallback, CpuFallbackError, FfprobeReport};

    fn fake_executable(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, format!("#!/bin/sh\nset -eu\n{body}\n")).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();
        path
    }

    #[test]
    fn controlled_child_collects_exact_arguments_and_drains_flooded_stderr() {
        let dir = tempfile::tempdir().unwrap();
        let captured = dir.path().join("arguments.txt");
        let script = format!(
            r#"
if [ "$1" = "-version" ]; then echo "ffmpeg version bounded-test"; exit 0; fi
if [ "$1" = "-v" ]; then
  printf '%s' '{{"format":{{"format_name":"matroska,webm","duration":"2"}},"streams":[{{"codec_name":"vp9","codec_type":"video","pix_fmt":"yuv420p"}},{{"codec_name":"opus","codec_type":"audio"}}]}}'
  yes x | head -c 131072 >&2
  exit 0
fi
printf '%s\n' "$@" > '{}'
yes x | head -c 131072 >&2
last=''; for argument in "$@"; do last="$argument"; done
printf synthetic > "$last"
printf 'speed=1.0x\nprogress=end\n'
"#,
            captured.display()
        );
        let fake = fake_executable(dir.path(), "fake-media", &script);
        let output = dir.path().join("fallback.webm");
        let fallback = CpuFallback::new(&fake, &fake);
        let generated = fallback.generate_synthetic(&output, 3).unwrap();
        assert_eq!(generated.average_speed, Some(1.0));
        assert_eq!(generated.ffmpeg_build_id, "ffmpeg version bounded-test");
        assert_eq!(
            fs::read_to_string(captured)
                .unwrap()
                .lines()
                .collect::<Vec<_>>(),
            fallback
                .ffmpeg_arguments(&output, 3)
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        );
        let report = FfprobeReport::read(&fake, &output).unwrap();
        assert_eq!(report.video_codec.as_deref(), Some("vp9"));
    }

    #[test]
    fn controlled_child_streams_arbitrarily_long_valid_progress_without_retaining_it() {
        let dir = tempfile::tempdir().unwrap();
        let script = r#"
if [ "$1" = "-version" ]; then echo "ffmpeg version streaming-test"; exit 0; fi
last=''; for argument in "$@"; do last="$argument"; done
printf synthetic > "$last"
i=0; while [ "$i" -lt 6000 ]; do
  printf 'speed=2.0x\nprogress=continue\n'
  i=$((i + 1))
done
printf 'speed=2.0x\nprogress=end\n'
"#;
        let fake = fake_executable(dir.path(), "long-progress", script);
        let generated = CpuFallback::new(&fake, &fake)
            .generate_synthetic(&dir.path().join("fallback.webm"), 3)
            .unwrap();
        assert_eq!(generated.average_speed, Some(2.0));
    }

    #[test]
    fn controlled_child_failure_and_timeout_are_redacted_and_reaped() {
        let dir = tempfile::tempdir().unwrap();
        let failure = fake_executable(
            dir.path(),
            "failure",
            "echo '/private/raw-stderr' >&2; exit 7",
        );
        let error = CpuFallback::new(&failure, &failure)
            .generate_synthetic(&dir.path().join("out.webm"), 1)
            .unwrap_err();
        assert!(matches!(error, CpuFallbackError::Failed));
        assert!(!error.to_string().contains("private"));

        let pid = dir.path().join("child.pid");
        let timeout = fake_executable(
            dir.path(),
            "timeout",
            &format!("echo $$ > '{}'; sleep 5", pid.display()),
        );
        let started = Instant::now();
        let error = CpuFallback::new(&timeout, &timeout)
            .with_timeouts(Duration::from_secs(2), Duration::from_secs(2))
            .generate_synthetic(&dir.path().join("timeout.webm"), 1)
            .unwrap_err();
        assert!(matches!(error, CpuFallbackError::TimedOut));
        assert!(started.elapsed() < Duration::from_secs(4));
        let child_pid = fs::read_to_string(pid).unwrap();
        assert!(
            !std::process::Command::new("kill")
                .args(["-0", child_pid.trim()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success()
        );
    }
}
