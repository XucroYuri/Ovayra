#![cfg(unix)]

use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

use spike_media::{Backend, FORCED_FAILURE_DEVICE};

fn executable(dir: &Path, name: &str, script: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\nset -eu\n{script}\n")).unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).unwrap();
    path
}

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_ovayra-spike")
}

#[test]
fn every_backend_forces_hardware_failure_then_runs_cpu_once() {
    for backend in Backend::ALL {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("calls.log");
        let ffmpeg = executable(
            dir.path(),
            "ffmpeg",
            &format!(
                r#"
printf '%s\n' "$*" >> '{}'
case " $* " in
  *" -ovayra_forced_hardware_failure "*) exit 9 ;;
  *" libvpx-vp9 "*) last=''; for x in "$@"; do last="$x"; done; printf x > "$last"; printf 'frame=1\nprogress=end\n'; exit 0 ;;
  *" -version "*) echo 'ffmpeg version fake'; exit 0 ;;
esac
exit 7
"#,
                log.display()
            ),
        );
        let ffprobe = executable(
            dir.path(),
            "ffprobe",
            r#"
printf '%s' '{"format":{"format_name":"matroska,webm","duration":"1"},"streams":[{"codec_name":"vp9","codec_type":"video","pix_fmt":"yuv420p"},{"codec_name":"opus","codec_type":"audio"}]}'
"#,
        );
        let output = dir.path().join("out.webm");
        let evidence = dir.path().join("evidence.json");
        let result = Command::new(binary())
            .args([
                "media",
                "forced-fallback",
                "--backend",
                backend.as_str(),
                "--ffmpeg",
            ])
            .arg(&ffmpeg)
            .args(["--ffprobe"])
            .arg(&ffprobe)
            .args(["--input"])
            .arg(dir.path().join("synthetic.mp4"))
            .args(["--output"])
            .arg(&output)
            .args(["--evidence"])
            .arg(&evidence)
            .env("OVAYRA_EVIDENCE_TARGET", "linux-x64-vaapi-wayland")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&result.stdout),
            "ACTUAL_BACKEND=cpu\nDOWNGRADE_OBSERVED=true\n"
        );
        let calls = fs::read_to_string(log).unwrap();
        let lines: Vec<_> = calls.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].contains(FORCED_FAILURE_DEVICE));
        assert!(lines[0].contains("-ovayra_forced_hardware_failure"));
        assert!(lines[1].contains("libvpx-vp9"));
        assert!(lines[2].contains("-version"));
        let evidence_text = fs::read_to_string(evidence).unwrap();
        assert!(evidence_text.contains("\"actual_backend\": \"cpu\""));
        assert!(evidence_text.contains("\"requested_backend\""));
        assert!(evidence_text.contains("\"downgrade_code\""));
    }
}

#[test]
fn surprising_hardware_success_never_runs_cpu_or_writes_pass_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("calls.log");
    let ffmpeg = executable(
        dir.path(),
        "ffmpeg",
        &format!(
            r#"
printf '%s\n' "$*" >> '{}'
last=''; for x in "$@"; do last="$x"; done; printf x > "$last"; printf 'frame=1\nprogress=end\n'
"#,
            log.display()
        ),
    );
    let ffprobe = executable(
        dir.path(),
        "ffprobe",
        r#"printf '%s' '{"format":{"duration":"1"},"streams":[{"codec_type":"video"}]}'"#,
    );
    let evidence = dir.path().join("evidence.json");
    let result = Command::new(binary())
        .args([
            "media",
            "forced-fallback",
            "--backend",
            "videotoolbox",
            "--ffmpeg",
        ])
        .arg(&ffmpeg)
        .args(["--ffprobe"])
        .arg(&ffprobe)
        .args(["--input"])
        .arg(dir.path().join("in.mp4"))
        .args(["--output"])
        .arg(dir.path().join("out.webm"))
        .args(["--evidence"])
        .arg(&evidence)
        .env("OVAYRA_EVIDENCE_TARGET", "linux-x64-vaapi-wayland")
        .output()
        .unwrap();
    assert!(!result.status.success());
    let calls = fs::read_to_string(log).unwrap();
    assert!(!calls.contains("libvpx-vp9"));
    assert!(!evidence.exists());
}
