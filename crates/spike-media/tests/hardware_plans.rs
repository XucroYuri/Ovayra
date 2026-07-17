use spike_media::{Backend, HardwarePlan, Inventory};

#[test]
fn videotoolbox_plan_uses_platform_decoder_and_encoder() {
    let plan = HardwarePlan::self_test(Backend::VideoToolbox);
    assert!(
        plan.args()
            .windows(2)
            .any(|w| w == ["-hwaccel", "videotoolbox"])
    );
    assert!(
        plan.args()
            .windows(2)
            .any(|w| w == ["-c:v", "h264_videotoolbox"])
    );
}

#[test]
fn vaapi_plan_keeps_frames_on_the_hardware_surface() {
    let plan = HardwarePlan::self_test(Backend::Vaapi);
    assert!(
        plan.args()
            .windows(2)
            .any(|w| w == ["-hwaccel_output_format", "vaapi"])
    );
    assert!(plan.args().windows(2).any(|w| w == ["-c:v", "h264_vaapi"]));
    assert!(
        plan.args()
            .windows(2)
            .any(|w| w == ["-vaapi_device", "/dev/dri/renderD128"])
    );
}

#[test]
fn all_plans_share_the_same_generated_ten_second_h264_aac_source() {
    let sources: Vec<_> = Backend::ALL
        .into_iter()
        .map(HardwarePlan::self_test)
        .map(|plan| plan.source_args().to_vec())
        .collect();
    assert!(sources.windows(2).all(|window| window[0] == window[1]));
    assert!(sources[0].windows(2).any(|w| w == ["-t", "10"]));
    assert!(sources[0].windows(2).any(|w| w == ["-c:v", "libx264"]));
    assert!(sources[0].windows(2).any(|w| w == ["-c:a", "aac"]));
}

#[test]
fn no_plan_claims_gpu_without_a_runtime_self_test() {
    for backend in Backend::ALL {
        assert!(HardwarePlan::self_test(backend).requires_observed_output());
    }
}

#[test]
fn availability_requires_inventory_success_and_an_observed_video_frame() {
    let plan = HardwarePlan::self_test(Backend::Vaapi);
    let inventory = Inventory::from_outputs(&[
        ("-hwaccels", "vaapi"),
        ("-decoders", "h264"),
        ("-encoders", "h264_vaapi"),
        ("-filters", "scale_vaapi"),
    ]);
    assert!(!plan.is_available(&inventory, true, 0));
    assert!(!plan.is_available(&inventory, false, 1));
    assert!(plan.is_available(&inventory, true, 1));
}

#[test]
fn availability_rejects_missing_required_inventory_components() {
    let plan = HardwarePlan::self_test(Backend::NvencNvdec);
    let inventory = Inventory::from_outputs(&[
        ("-hwaccels", "cuda"),
        ("-decoders", "h264_cuvid"),
        ("-encoders", "h264_nvenc"),
    ]);
    assert!(!plan.is_available(&inventory, true, 1));
}

#[test]
fn availability_does_not_treat_a_partial_inventory_name_as_a_component() {
    let plan = HardwarePlan::self_test(Backend::Vaapi);
    let inventory = Inventory::from_outputs(&[
        ("-hwaccels", "vaapi-compatible"),
        ("-decoders", "h264"),
        ("-encoders", "h264_vaapi"),
        ("-filters", "scale_vaapi"),
    ]);
    assert!(!plan.is_available(&inventory, true, 1));
}
