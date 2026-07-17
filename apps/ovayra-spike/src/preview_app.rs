use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use slint::{ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer, Weak};
use spike_contracts::{Evidence, SpikeId, TargetId, Verdict};
use spike_media::{FfmpegPreview, Frame, LatestFrame};
use sysinfo::{ProcessesToUpdate, System, get_current_pid};

slint::include_modules!();

const LATENCY_SAMPLE_LIMIT: usize = 4096;

/// Testable single-pending-closure state shared by background publishers and the UI loop.
#[derive(Default)]
pub(crate) struct ApplySchedule {
    queued: AtomicBool,
}

impl ApplySchedule {
    pub(crate) fn try_queue(&self) -> bool {
        !self.queued.swap(true, Ordering::AcqRel)
    }

    pub(crate) fn complete(&self) {
        self.queued.store(false, Ordering::Release);
    }

    fn enter_ui_closure(&self) {
        self.queued.store(false, Ordering::Release);
    }
}

#[derive(Default)]
struct Metrics {
    frames_read: u64,
    frames_applied: u64,
    latencies: VecDeque<Duration>,
    event_loop_errors: u64,
    hidden: bool,
    restored: bool,
    rss_samples: Vec<(u64, u64)>,
}

impl Metrics {
    fn record_applied(&mut self, frame: &Frame) {
        self.frames_applied = self.frames_applied.saturating_add(1);
        if self.latencies.len() == LATENCY_SAMPLE_LIMIT {
            let _ = self.latencies.pop_front();
        }
        self.latencies.push_back(frame.enqueued_at().elapsed());
    }

    fn percentile_millis(&self, percentile: u8) -> u64 {
        let mut samples: Vec<u64> = self
            .latencies
            .iter()
            .map(|latency| latency.as_millis().try_into().unwrap_or(u64::MAX))
            .collect();
        if samples.is_empty() {
            return 0;
        }
        samples.sort_unstable();
        let index = (samples.len() - 1) * usize::from(percentile) / 100;
        samples[index]
    }
}

/// Main-thread-only Slint bridge. Workers only submit `Frame` values to it.
#[derive(Clone)]
struct FrameBridge {
    latest: LatestFrame,
    schedule: Arc<ApplySchedule>,
    ui: Weak<PreviewWindow>,
    metrics: Arc<Mutex<Metrics>>,
    event_loop_errors: Arc<AtomicU64>,
}

impl FrameBridge {
    fn new(ui: Weak<PreviewWindow>, metrics: Arc<Mutex<Metrics>>) -> Self {
        Self {
            latest: LatestFrame::default(),
            schedule: Arc::new(ApplySchedule::default()),
            ui,
            metrics,
            event_loop_errors: Arc::new(AtomicU64::new(0)),
        }
    }

    fn publish(&self, frame: Frame) {
        self.latest.publish(frame);
        self.schedule_apply();
    }

    fn schedule_apply(&self) {
        if self.schedule.try_queue() {
            self.queue_apply();
        }
    }

    fn queue_apply(&self) {
        let latest = self.latest.clone();
        let schedule = Arc::clone(&self.schedule);
        let ui = self.ui.clone();
        let metrics = Arc::clone(&self.metrics);
        let errors = Arc::clone(&self.event_loop_errors);
        if slint::invoke_from_event_loop(move || {
            schedule.enter_ui_closure();
            if let (Some(handle), Some(frame)) = (ui.upgrade(), latest.take()) {
                let pixels = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                    frame.bytes(),
                    frame.width().try_into().expect("validated frame width"),
                    frame.height().try_into().expect("validated frame height"),
                );
                let latency = frame.enqueued_at().elapsed().as_millis();
                if let Ok(mut metric) = metrics.lock() {
                    metric.record_applied(&frame);
                }
                handle.set_preview_frame(Image::from_rgba8(pixels));
                handle.set_metrics_text(format!("applied; enqueue-to-apply={latency} ms").into());
            }
        })
        .is_err()
        {
            errors.fetch_add(1, Ordering::Relaxed);
            self.schedule.complete();
        }
    }
}

pub(crate) fn run_preview(
    ffmpeg: PathBuf,
    input: PathBuf,
    duration_seconds: u64,
    automation: bool,
    evidence_path: &Path,
    target: TargetId,
) -> Result<()> {
    let started = Instant::now();
    let window = PreviewWindow::new().context("unable to create preview window")?;
    let tray = SpikeTray::new().context("unable to create technical tray icon")?;
    let metrics = Arc::new(Mutex::new(Metrics::default()));
    let bridge = FrameBridge::new(window.as_weak(), Arc::clone(&metrics));
    let cancelled = Arc::new(AtomicBool::new(false));
    let stream_errors = Arc::new(AtomicU64::new(0));

    let restore_window = window.as_weak();
    tray.on_restore(move || {
        if let Some(window) = restore_window.upgrade() {
            let _ = window.show();
        }
    });
    tray.on_quit(|| {
        let _ = slint::quit_event_loop();
    });
    window
        .window()
        .on_close_requested(|| slint::CloseRequestResponse::HideWindow);

    let reader_bridge = bridge.clone();
    let reader_cancel = Arc::clone(&cancelled);
    let reader_errors = Arc::clone(&stream_errors);
    let reader = thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build();
        if let Ok(runtime) = runtime {
            match runtime.block_on(FfmpegPreview::new(ffmpeg).stream_with(
                &input,
                reader_cancel,
                Duration::from_secs(duration_seconds.saturating_add(5)),
                move |frame| {
                    if let Ok(mut metrics) = reader_bridge.metrics.lock() {
                        metrics.frames_read = metrics.frames_read.saturating_add(1);
                    }
                    reader_bridge.publish(frame);
                },
            )) {
                Err(spike_media::PreviewError::Cancelled) if automation => {}
                Err(_) => {
                    reader_errors.fetch_add(1, Ordering::Relaxed);
                }
                Ok(_) => {}
            }
        } else {
            reader_errors.fetch_add(1, Ordering::Relaxed);
        }
    });

    window.show().context("unable to show preview window")?;
    if automation {
        schedule_automation(
            window.as_weak(),
            Arc::clone(&cancelled),
            &metrics,
            duration_seconds,
        );
    }
    let event_loop = slint::run_event_loop_until_quit();
    cancelled.store(true, Ordering::Release);
    let _ = reader.join();
    if event_loop.is_err() {
        bridge.event_loop_errors.fetch_add(1, Ordering::Relaxed);
    }

    let mut metrics = metrics
        .lock()
        .map_err(|_| anyhow::anyhow!("preview metrics poisoned"))?;
    metrics.event_loop_errors = bridge.event_loop_errors.load(Ordering::Relaxed);
    sample_rss(&mut metrics, duration_seconds);
    finish_preview_evidence(
        &metrics,
        bridge.latest.dropped_frames(),
        stream_errors.load(Ordering::Relaxed),
        target,
        started,
        evidence_path,
        automation,
    )
}

fn finish_preview_evidence(
    metrics: &Metrics,
    dropped: u64,
    preview_stream_errors: u64,
    target: TargetId,
    started: Instant,
    evidence_path: &Path,
    automation: bool,
) -> Result<()> {
    let p95 = metrics.percentile_millis(95);
    let rss_growth_mib = rss_growth_mib(&metrics.rss_samples);
    let automation_passed = !automation || (metrics.hidden && metrics.restored);
    let passed = metrics.frames_read > 0
        && metrics.frames_applied > 0
        && automation_passed
        && p95 <= 100
        && rss_growth_mib <= 64
        && metrics.event_loop_errors == 0
        && preview_stream_errors == 0;
    let mut evidence = Evidence::new(SpikeId::Preview, target);
    evidence.measure("frames_read", metrics.frames_read)?;
    evidence.measure("frames_applied", metrics.frames_applied)?;
    evidence.measure("frames_dropped", dropped)?;
    evidence.measure("p50_ms", metrics.percentile_millis(50))?;
    evidence.measure("p95_ms", p95)?;
    evidence.measure("p99_ms", metrics.percentile_millis(99))?;
    evidence.measure("rss_samples_bytes", &metrics.rss_samples)?;
    evidence.measure("rss_growth_mib", rss_growth_mib)?;
    evidence.measure("renderer_backend", configured_slint_backend())?;
    evidence.measure("automation_hide", metrics.hidden)?;
    evidence.measure("automation_restore", metrics.restored)?;
    evidence.measure("event_loop_errors", metrics.event_loop_errors)?;
    evidence.measure("preview_stream_errors", preview_stream_errors)?;
    evidence.finish(
        if passed { Verdict::Pass } else { Verdict::Fail },
        started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
    );
    super::write_finished_evidence(evidence_path, &evidence)?;
    if passed {
        println!("PREVIEW=PASS fps=24 p95_ms={p95} rss_growth_mib={rss_growth_mib}");
        Ok(())
    } else {
        anyhow::bail!("PREVIEW=FAIL p95_ms={p95} rss_growth_mib={rss_growth_mib}")
    }
}

fn schedule_automation(
    window: Weak<PreviewWindow>,
    cancelled: Arc<AtomicBool>,
    metrics: &Arc<Mutex<Metrics>>,
    duration_seconds: u64,
) {
    let hide_window = window.clone();
    let hide_metrics = Arc::clone(metrics);
    slint::Timer::single_shot(Duration::from_millis(100), move || {
        if let Some(window) = hide_window.upgrade()
            && window.hide().is_ok()
            && let Ok(mut metrics) = hide_metrics.lock()
        {
            metrics.hidden = true;
        }
    });
    let restore_metrics = Arc::clone(metrics);
    slint::Timer::single_shot(Duration::from_millis(250), move || {
        if let Some(window) = window.upgrade()
            && window.show().is_ok()
            && let Ok(mut metrics) = restore_metrics.lock()
        {
            metrics.restored = true;
        }
    });
    let finish_metrics = Arc::clone(metrics);
    slint::Timer::single_shot(Duration::from_secs(20), move || {
        if let Ok(mut metrics) = finish_metrics.lock() {
            sample_rss(&mut metrics, 20);
        }
    });
    slint::Timer::single_shot(Duration::from_secs(duration_seconds), move || {
        cancelled.store(true, Ordering::Release);
        let _ = slint::quit_event_loop();
    });
}

fn sample_rss(metrics: &mut Metrics, seconds: u64) {
    let Ok(pid) = get_current_pid() else {
        return;
    };
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    if let Some(process) = system.process(pid) {
        metrics.rss_samples.push((seconds, process.memory()));
    }
}

fn rss_growth_mib(samples: &[(u64, u64)]) -> u64 {
    let Some((_, first)) = samples.first() else {
        return u64::MAX;
    };
    let Some((_, last)) = samples.last() else {
        return u64::MAX;
    };
    last.saturating_sub(*first) / (1024 * 1024)
}

fn configured_slint_backend() -> String {
    std::env::var("SLINT_BACKEND").unwrap_or_else(|_| "auto".to_owned())
}

#[cfg(test)]
mod tests {
    use super::ApplySchedule;

    #[test]
    fn scheduler_permits_exactly_one_pending_ui_closure() {
        let schedule = ApplySchedule::default();
        assert!(schedule.try_queue());
        assert!(!schedule.try_queue());
        schedule.complete();
        assert!(schedule.try_queue());
    }
}
