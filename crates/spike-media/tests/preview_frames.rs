use std::{
    sync::{Arc, Barrier},
    time::Duration,
};

use spike_media::{Frame, FrameError, LatestFrame};

#[test]
fn rgba_frame_requires_an_exact_checked_byte_count() {
    assert!(matches!(
        Frame::rgba(usize::MAX, 2, Vec::new(), 1),
        Err(FrameError::DimensionOverflow { .. })
    ));
    assert!(Frame::rgba(2, 2, vec![0; 15], 1).is_err());
    assert!(Frame::rgba(2, 2, vec![0; 16], 1).is_ok());
}

#[test]
fn frame_enqueue_times_and_latency_are_monotonic() {
    let first = Frame::rgba(1, 1, vec![0; 4], 1).unwrap();
    std::thread::sleep(Duration::from_millis(2));
    let second = Frame::rgba(1, 1, vec![0; 4], 2).unwrap();

    assert!(second.enqueued_at() >= first.enqueued_at());
    assert!(first.enqueue_latency() >= Duration::from_millis(2));
}

#[test]
fn latest_frame_replaces_stale_work_and_counts_drops() {
    let slot = LatestFrame::default();
    slot.publish(Frame::rgba(1, 1, vec![0, 0, 0, 255], 1).unwrap());
    slot.publish(Frame::rgba(1, 1, vec![1, 0, 0, 255], 2).unwrap());

    assert_eq!(slot.dropped_frames(), 1);
    assert_eq!(slot.take().unwrap().sequence(), 2);
    assert!(slot.take().is_none());
}

#[test]
fn latest_frame_is_a_single_slot_under_concurrent_publishers() {
    const PUBLISHERS: usize = 16;
    let slot = LatestFrame::default();
    let barrier = Arc::new(Barrier::new(PUBLISHERS));
    let mut publishers = Vec::new();
    for sequence in 0..PUBLISHERS {
        let slot = slot.clone();
        let barrier = barrier.clone();
        publishers.push(std::thread::spawn(move || {
            barrier.wait();
            slot.publish(
                Frame::rgba(
                    1,
                    1,
                    vec![
                        u8::try_from(sequence).expect("publisher count must fit in a byte"),
                        0,
                        0,
                        255,
                    ],
                    u64::try_from(sequence).expect("publisher count must fit in u64"),
                )
                .unwrap(),
            );
        }));
    }
    for publisher in publishers {
        publisher.join().unwrap();
    }

    assert!(slot.take().is_some());
    assert!(slot.take().is_none());
    assert_eq!(slot.dropped_frames(), (PUBLISHERS - 1) as u64);
}

#[cfg(unix)]
mod child_processes {
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
        process::Stdio,
        sync::{Arc, atomic::AtomicBool},
        time::{Duration, Instant},
    };

    use spike_media::{
        FfmpegPreview, LatestFrame, PREVIEW_FRAME_BYTES, PREVIEW_HEIGHT, PREVIEW_WIDTH,
        PreviewError,
    };

    fn fake_executable(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, format!("#!/bin/sh\nset -eu\n{body}\n")).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();
        path
    }

    #[tokio::test]
    async fn preview_uses_the_exact_raw_rgba_ffmpeg_argv() {
        let dir = tempfile::tempdir().unwrap();
        let captured = dir.path().join("argv.txt");
        let fake = fake_executable(
            dir.path(),
            "argv",
            &format!("printf '%s\\n' \"$@\" > '{}'", captured.display()),
        );
        let preview = FfmpegPreview::new(fake);
        let slot = LatestFrame::default();
        preview
            .stream(
                Path::new("target/phase-0/fallback.webm"),
                slot,
                Arc::new(AtomicBool::new(false)),
                Duration::from_secs(5),
            )
            .await
            .unwrap();

        assert_eq!(
            fs::read_to_string(captured)
                .unwrap()
                .lines()
                .collect::<Vec<_>>(),
            [
                "-hide_banner",
                "-nostdin",
                "-re",
                "-stream_loop",
                "-1",
                "-i",
                "target/phase-0/fallback.webm",
                "-an",
                "-vf",
                "scale=640:360,fps=24",
                "-pix_fmt",
                "rgba",
                "-f",
                "rawvideo",
                "pipe:1",
            ]
        );
    }

    #[tokio::test]
    async fn reader_distinguishes_clean_eof_from_partial_frame_errors() {
        let dir = tempfile::tempdir().unwrap();
        let empty = fake_executable(dir.path(), "empty", "exit 0");
        let partial = fake_executable(dir.path(), "partial", "printf short");
        let cancelled = Arc::new(AtomicBool::new(false));

        let result = FfmpegPreview::new(empty)
            .stream(
                Path::new("input.webm"),
                LatestFrame::default(),
                cancelled.clone(),
                Duration::from_secs(5),
            )
            .await
            .unwrap();
        assert_eq!(result.frames_read, 0);
        assert!(matches!(
            FfmpegPreview::new(partial)
                .stream(
                    Path::new("input.webm"),
                    LatestFrame::default(),
                    cancelled,
                    Duration::from_secs(5)
                )
                .await,
            Err(PreviewError::PartialFrame { received: 5 })
        ));
    }

    #[tokio::test]
    async fn preview_drains_bounded_stderr_while_publishing_raw_frames() {
        let dir = tempfile::tempdir().unwrap();
        let fake = fake_executable(
            dir.path(),
            "stderr",
            "yes diagnostic | head -c 2097152 >&2\ndd if=/dev/zero bs=921600 count=1 2>/dev/null",
        );
        let slot = LatestFrame::default();
        let result = FfmpegPreview::new(fake)
            .stream(
                Path::new("input.webm"),
                slot.clone(),
                Arc::new(AtomicBool::new(false)),
                Duration::from_secs(5),
            )
            .await
            .unwrap();

        assert_eq!(result.frames_read, 1);
        assert!(result.stderr_bytes <= 64 * 1024);
        let frame = slot.take().unwrap();
        assert_eq!(
            (frame.width(), frame.height(), frame.bytes().len()),
            (PREVIEW_WIDTH, PREVIEW_HEIGHT, PREVIEW_FRAME_BYTES)
        );
    }

    #[tokio::test]
    async fn timeout_kills_and_reaps_the_preview_child() {
        let dir = tempfile::tempdir().unwrap();
        let pid = dir.path().join("child.pid");
        let fake = fake_executable(
            dir.path(),
            "hang",
            &format!("echo $$ > '{}'\nprintf x\nexec sleep 5", pid.display()),
        );
        let started = Instant::now();
        let result = FfmpegPreview::new(fake)
            .stream(
                Path::new("input.webm"),
                LatestFrame::default(),
                Arc::new(AtomicBool::new(false)),
                Duration::from_secs(5),
            )
            .await;

        assert!(matches!(result, Err(PreviewError::TimedOut)));
        assert!(started.elapsed() < Duration::from_secs(6));
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

    #[tokio::test]
    async fn cancellation_kills_and_reaps_a_blocked_preview_child() {
        let dir = tempfile::tempdir().unwrap();
        let pid = dir.path().join("cancelled.pid");
        let fake = fake_executable(
            dir.path(),
            "cancel",
            &format!(
                "echo $$ > '{}'\ndd if=/dev/zero bs=921600 count=1 2>/dev/null\nexec sleep 5",
                pid.display()
            ),
        );
        let cancelled = Arc::new(AtomicBool::new(false));
        let trigger = cancelled.clone();

        let result = FfmpegPreview::new(fake)
            .stream_with(
                Path::new("input.webm"),
                cancelled,
                Duration::from_secs(10),
                move |_| trigger.store(true, std::sync::atomic::Ordering::Release),
            )
            .await;

        assert!(matches!(result, Err(PreviewError::Cancelled)));
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
