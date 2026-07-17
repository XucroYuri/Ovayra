use std::time::Duration;

use spike_platform::{GroupedProcess, ProcessTreeProbe};

const SHORT_TIMEOUT: Duration = Duration::from_millis(250);
const PROCESS_TIMEOUT: Duration = Duration::from_secs(5);

#[tokio::test]
async fn cancellation_terminates_parent_and_grandchild() {
    let helper = env!("CARGO_BIN_EXE_ovayra-spike");
    let mut process = GroupedProcess::spawn(helper, &["child-tree"])
        .await
        .unwrap();
    let tree = process
        .wait_for_reported_tree(PROCESS_TIMEOUT)
        .await
        .unwrap();

    process.kill_and_wait(PROCESS_TIMEOUT).await.unwrap();

    assert!(!ProcessTreeProbe::any_alive(&tree));
}

#[tokio::test]
async fn malformed_tree_report_is_bounded_and_cleans_up_the_group() {
    let helper = env!("CARGO_BIN_EXE_ovayra-spike");
    let mut process = GroupedProcess::spawn(helper, &["child-tree", "--malformed-report"])
        .await
        .unwrap();
    let leader = process.leader();

    let error = process
        .wait_for_reported_tree(PROCESS_TIMEOUT)
        .await
        .unwrap_err();

    assert!(error.to_string().contains("invalid child-tree JSON report"));
    assert!(!ProcessTreeProbe::is_alive(&leader));
}

#[tokio::test]
async fn missing_tree_report_times_out_and_drop_reaps_the_group() {
    let helper = env!("CARGO_BIN_EXE_ovayra-spike");
    let mut process = GroupedProcess::spawn(helper, &["child-tree", "--delay-report"])
        .await
        .unwrap();
    let leader = process.leader();

    let error = process
        .wait_for_reported_tree(SHORT_TIMEOUT)
        .await
        .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("timed out waiting for child-tree JSON report")
    );
    drop(process);
    tokio::time::timeout(PROCESS_TIMEOUT, async {
        while ProcessTreeProbe::is_alive(&leader) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("drop must kill and reap the process group");
}

#[tokio::test]
async fn cleanup_of_an_already_exited_group_returns_without_hanging() {
    let helper = env!("CARGO_BIN_EXE_ovayra-spike");
    let mut process = GroupedProcess::spawn(helper, &["child-tree", "--exit-before-report"])
        .await
        .unwrap();

    let result =
        tokio::time::timeout(PROCESS_TIMEOUT, process.kill_and_wait(PROCESS_TIMEOUT)).await;

    assert!(
        result.is_ok(),
        "cleanup must be bounded even when the child already exited"
    );
}
