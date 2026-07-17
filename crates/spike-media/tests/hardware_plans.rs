use spike_media::{Backend, HardwarePlan, Inventory, InventoryCommand, InventoryOutput};

fn complete_inventory() -> Inventory {
    Inventory::from_command_outputs(&[
        InventoryOutput::success(InventoryCommand::Version, "version"),
        InventoryOutput::success(InventoryCommand::Buildconf, "buildconf"),
        InventoryOutput::success(
            InventoryCommand::Hwaccels,
            "vaapi cuda videotoolbox d3d11va",
        ),
        InventoryOutput::success(InventoryCommand::Decoders, "h264 h264_cuvid"),
        InventoryOutput::success(
            InventoryCommand::Encoders,
            "h264_vaapi h264_nvenc h264_videotoolbox h264_mf",
        ),
        InventoryOutput::success(InventoryCommand::Filters, "scale scale_vaapi scale_cuda"),
    ])
    .unwrap()
}

#[test]
fn inventory_commands_are_the_exact_required_six_once_each() {
    assert_eq!(
        InventoryCommand::ALL.map(InventoryCommand::args),
        [
            ["-version"],
            ["-buildconf"],
            ["-hwaccels"],
            ["-decoders"],
            ["-encoders"],
            ["-filters"]
        ]
    );
}

#[test]
fn inventory_rejects_missing_or_failed_command_outputs() {
    let complete = [
        InventoryOutput::success(InventoryCommand::Version, "version"),
        InventoryOutput::success(InventoryCommand::Buildconf, "buildconf"),
        InventoryOutput::success(InventoryCommand::Hwaccels, "vaapi"),
        InventoryOutput::success(InventoryCommand::Decoders, "h264"),
        InventoryOutput::success(InventoryCommand::Encoders, "h264_vaapi"),
        InventoryOutput::success(InventoryCommand::Filters, "scale_vaapi"),
    ];
    assert!(Inventory::from_command_outputs(&complete[..5]).is_err());
    let mut failed = complete;
    failed[3] = InventoryOutput::failed(InventoryCommand::Decoders, "unavailable");
    assert!(Inventory::from_command_outputs(&failed).is_err());
}

#[test]
fn inventory_output_cap_is_byte_strict_at_a_utf8_boundary() {
    let input = format!("{}é", "x".repeat(65_535));
    let output = InventoryOutput::success(InventoryCommand::Version, input);
    assert_eq!(output.byte_len(), 65_535);
    let inventory = Inventory::from_command_outputs(&[
        output,
        InventoryOutput::success(InventoryCommand::Buildconf, "buildconf"),
        InventoryOutput::success(InventoryCommand::Hwaccels, "vaapi"),
        InventoryOutput::success(InventoryCommand::Decoders, "h264"),
        InventoryOutput::success(InventoryCommand::Encoders, "h264_vaapi"),
        InventoryOutput::success(InventoryCommand::Filters, "scale_vaapi"),
    ]);
    assert!(inventory.is_ok());
}

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
    let inventory = complete_inventory();
    assert!(!plan.is_available(&inventory, true, 0));
    assert!(!plan.is_available(&inventory, false, 1));
    assert!(plan.is_available(&inventory, true, 1));
}

#[test]
fn availability_rejects_missing_required_inventory_components() {
    let inventory = Inventory::from_command_outputs(&[
        InventoryOutput::success(InventoryCommand::Version, "version"),
        InventoryOutput::success(InventoryCommand::Buildconf, "buildconf"),
        InventoryOutput::success(InventoryCommand::Hwaccels, "cuda"),
        InventoryOutput::success(InventoryCommand::Decoders, "h264_cuvid"),
        InventoryOutput::success(InventoryCommand::Encoders, "h264_nvenc"),
    ]);
    assert!(inventory.is_err());
}

#[test]
fn availability_does_not_treat_a_partial_inventory_name_as_a_component() {
    let plan = HardwarePlan::self_test(Backend::Vaapi);
    let inventory = Inventory::from_command_outputs(&[
        InventoryOutput::success(InventoryCommand::Version, "version"),
        InventoryOutput::success(InventoryCommand::Buildconf, "buildconf"),
        InventoryOutput::success(InventoryCommand::Hwaccels, "vaapi-compatible"),
        InventoryOutput::success(InventoryCommand::Decoders, "h264"),
        InventoryOutput::success(InventoryCommand::Encoders, "h264_vaapi"),
        InventoryOutput::success(InventoryCommand::Filters, "scale_vaapi"),
    ])
    .unwrap();
    assert!(!plan.is_available(&inventory, true, 1));
}
