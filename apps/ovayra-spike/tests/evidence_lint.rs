use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use std::path::Path;

use spike_contracts::{PhaseZeroProof, PreviewProof, TargetId};

static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: std::path::PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before epoch")
            .as_nanos();
        let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("ovayra-evidence-lint-{unique}-{sequence}"));
        fs::create_dir_all(&root).expect("fixture directory");
        Self { root }
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().expect("parent")).expect("nested fixture directory");
        fs::write(path, contents).expect("fixture file");
    }

    fn run(&self, text: bool) -> std::process::Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ovayra-spike"));
        command.args(["evidence", "lint", "--dir"]);
        command.arg(&self.root);
        if text {
            command.arg("--text");
        }
        command.output().expect("run evidence linter")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn assert_rejected(output: &std::process::Output, category: &str, secret: &str) {
    assert!(!output.status.success(), "lint unexpectedly succeeded");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("entry="),
        "missing redacted entry id: {stderr}"
    );
    assert!(stderr.contains(category), "missing category: {stderr}");
    assert!(
        !stderr.contains(secret),
        "secret leaked into diagnostics: {stderr}"
    );
}

#[test]
fn lint_accepts_a_finished_typed_evidence_document() {
    let fixture = Fixture::new();
    fixture.write(".gitkeep", "");
    fixture.write(
        "nested/valid.json",
        r#"{"schema_version":1,"spike":"preview","target":"macos-arm64-vt","verdict":"pass","duration_ms":120000,"measurements":{"fps":24},"observations":["tray restored"]}"#,
    );
    assert!(fixture.run(false).status.success());
}

#[test]
fn lint_rejects_sensitive_key_names_at_any_depth_case_insensitively() {
    let fixture = Fixture::new();
    fixture.write(
        "nested/keys.json",
        r#"{"safe":{"API_Key":"AIzaSensitive"}}"#,
    );
    assert_rejected(&fixture.run(false), "forbidden-key", "AIzaSensitive");
}

#[test]
fn lint_rejects_every_sensitive_known_field_spelling() {
    for key in [
        "api_key",
        "accessToken",
        "client_secret",
        "PASSWORD",
        "upload-url",
        "Prompt",
        "model_result",
        "media path",
        "file.name",
    ] {
        let fixture = Fixture::new();
        fixture.write("fields.json", &format!(r#"{{"safe":{{"{key}":1}}}}"#));
        assert_rejected(&fixture.run(false), "forbidden-key", key);
    }
}

#[test]
fn lint_accepts_an_empty_initialized_evidence_directory() {
    let fixture = Fixture::new();
    assert!(fixture.run(false).status.success());
}

#[test]
fn lint_rejects_duplicate_json_keys() {
    let fixture = Fixture::new();
    fixture.write(
        "duplicate.json",
        r#"{"one":"rawduplicatecontent","one":"rawduplicatecontent"}"#,
    );
    assert_rejected(&fixture.run(false), "duplicate-key", "rawduplicatecontent");
}

#[test]
fn lint_rejects_credential_and_transport_values_without_echoing_them() {
    let cases = [
        ("api.json", r#"{"note":"AiZaSecretValue"}"#, "api-key"),
        (
            "bearer.json",
            r#"{"note":"Authorization: Bearer SensitiveValue"}"#,
            "api-key",
        ),
        (
            "header.json",
            r#"{"note":"X-GoOg-UpLoAd-Protocol: resumable"}"#,
            "upload-header",
        ),
        (
            "private.json",
            r#"{"note":"-----BEGIN PRIVATE KEY-----"}"#,
            "private-key",
        ),
        (
            "upload.json",
            r#"{"note":"https://upload.example.test/path"}"#,
            "upload-url",
        ),
        (
            "query.json",
            r#"{"note":"https://safe.example.test/a?token=hidden"}"#,
            "url-credential",
        ),
        (
            "userinfo.json",
            r#"{"note":"https://name:pass@safe.example.test/a"}"#,
            "url-credential",
        ),
        (
            "fragment.json",
            r#"{"note":"https://safe.example.test/a#hidden"}"#,
            "url-credential",
        ),
        (
            "mac.json",
            r#"{"note":"/Users/alice/private"}"#,
            "home-path",
        ),
        (
            "linux.json",
            r#"{"note":"/home/alice/private"}"#,
            "home-path",
        ),
        (
            "windows.json",
            r#"{"note":"C:\\Users\\alice\\private"}"#,
            "home-path",
        ),
    ];
    for (file, contents, category) in cases {
        let fixture = Fixture::new();
        fixture.write(file, contents);
        assert_rejected(&fixture.run(false), category, "hidden");
    }
}

#[test]
fn lint_text_mode_scans_logs_and_rejects_binary_content() {
    let fixture = Fixture::new();
    fixture.write("package.log", "notary path /home/alice/private");
    assert_rejected(&fixture.run(true), "home-path", "alice");

    let binary = fixture.root.join("binary.log");
    fs::write(&binary, [0_u8, 1, 2]).expect("binary log");
    assert_rejected(&fixture.run(true), "binary-or-invalid-text", "\0");
}

#[test]
fn lint_fails_closed_for_missing_directories_and_non_regular_paths() {
    let missing = std::env::temp_dir().join("ovayra-evidence-lint-definitely-missing");
    let output = Command::new(env!("CARGO_BIN_EXE_ovayra-spike"))
        .args(["evidence", "lint", "--dir"])
        .arg(missing)
        .output()
        .expect("run missing directory lint");
    assert_rejected(&output, "missing-directory", "definitely-missing");

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::new();
        fixture.write("valid.json", "{}");
        symlink(Path::new("valid.json"), fixture.root.join("link.json")).expect("symlink");
        assert_rejected(&fixture.run(false), "symlink", "valid");
    }
}

#[test]
fn preview_verifier_requires_typed_measurements_instead_of_console_output() {
    let fixture = Fixture::new();
    let target = TargetId::new("macos-arm64-vt").unwrap();
    let proof = PhaseZeroProof::preview(
        &target,
        "production-test",
        &PreviewProof {
            requested_duration_ms: 120_000,
            measured_duration_ms: 120_000,
            milli_fps: 24_000,
            p95_ms: 100,
            rss_growth_mib: 64,
            frames_read: 2_880,
            frames_applied: 2_880,
            frames_dropped: 0,
            hidden: true,
            restored: true,
            event_loop_errors: 0,
            stream_errors: 0,
            renderer: "production-test".to_owned(),
        },
    );
    fixture.write("preview.json", &proof.to_pretty_json().unwrap());
    let output = Command::new(env!("CARGO_BIN_EXE_ovayra-spike"))
        .args([
            "evidence",
            "verify-preview",
            "--expected-target",
            "macos-arm64-vt",
            "--file",
        ])
        .arg(fixture.root.join("preview.json"))
        .output()
        .expect("run preview verifier");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn preview_verifier_rejects_wrong_target_zero_frames_and_nonzero_errors() {
    let fixture = Fixture::new();
    let cases = [
        ("wrong-target.json", "windows-x64-mf", 1, 1, 0, 0),
        ("zero-frames.json", "macos-arm64-vt", 0, 1, 0, 0),
        ("loop-error.json", "macos-arm64-vt", 1, 1, 1, 0),
        ("stream-error.json", "macos-arm64-vt", 1, 1, 0, 1),
    ];
    for (file, target, read, applied, loop_errors, stream_errors) in cases {
        fixture.write(file, &format!(r#"{{"schema_version":1,"spike":"preview","target":"{target}","verdict":"pass","duration_ms":120000,"measurements":{{"observed_milli_fps":24000,"requested_duration_seconds":120,"measured_elapsed_ms":120000,"frames_read":{read},"frames_applied":{applied},"automation_hide":true,"automation_restore":true,"p95_ms":100,"rss_growth_mib":64,"rss_samples_complete":true,"event_loop_errors":{loop_errors},"preview_stream_errors":{stream_errors}}},"observations":[]}}"#));
        let output = Command::new(env!("CARGO_BIN_EXE_ovayra-spike"))
            .args([
                "evidence",
                "verify-preview",
                "--expected-target",
                "macos-arm64-vt",
                "--file",
            ])
            .arg(fixture.root.join(file))
            .output()
            .expect("run preview verifier");
        assert!(!output.status.success(), "{file} unexpectedly accepted");
    }
}

#[test]
fn lint_rejects_unicode_confusable_keys_private_key_variants_and_secret_filenames() {
    let cases = [
        (
            "fullwidth.json",
            r#"{"ａｐｉ＿ｋｅｙ":"x"}"#,
            false,
            "forbidden-key",
            "fullwidth",
        ),
        (
            "escaped.json",
            r#"{"\uff41\uff50\uff49\uff3f\uff4b\uff45\uff59":"x"}"#,
            false,
            "forbidden-key",
            "escaped",
        ),
        (
            "armored.json",
            r#"{"note":"-----BEGIN PGP PRIVATE KEY BLOCK-----"}"#,
            false,
            "private-key",
            "armored",
        ),
        (
            "private.log",
            "-----BEGIN OPENSSH PRIVATE KEY-----",
            true,
            "private-key",
            "OPENSSH",
        ),
        (
            "AIzaSecret-name.json",
            "{}",
            false,
            "unsafe-name",
            "AIzaSecret-name",
        ),
    ];
    for (file, contents, text, category, secret) in cases {
        let fixture = Fixture::new();
        fixture.write(file, contents);
        assert_rejected(&fixture.run(text), category, secret);
    }
}

#[test]
fn lint_bounds_recursive_depth_and_directory_count() {
    let fixture = Fixture::new();
    let mut nested = String::new();
    for _ in 0..40 {
        nested.push_str("deep/");
    }
    fixture.write(&format!("{nested}evidence.log"), "safe");
    assert_rejected(&fixture.run(true), "max-depth", "deep");

    let fixture = Fixture::new();
    for index in 0..130 {
        fs::create_dir(fixture.root.join(format!("dir-{index}"))).expect("directory");
    }
    assert_rejected(&fixture.run(true), "too-many-directories", "dir-");
}
