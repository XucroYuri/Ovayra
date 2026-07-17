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

#[tokio::test]
async fn direct_drop_reaps_a_reported_parent_and_grandchild_without_a_tokio_runtime() {
    let helper = env!("CARGO_BIN_EXE_ovayra-spike");
    let mut process = GroupedProcess::spawn(helper, &["child-tree"])
        .await
        .unwrap();
    let tree = process
        .wait_for_reported_tree(PROCESS_TIMEOUT)
        .await
        .unwrap();

    std::thread::spawn(move || drop(process)).join().unwrap();

    wait_until_tree_is_dead(&tree).await;
}

#[tokio::test]
async fn direct_drop_reaps_an_already_exited_group() {
    let helper = env!("CARGO_BIN_EXE_ovayra-spike");
    let process = GroupedProcess::spawn(helper, &["child-tree", "--exit-before-report"])
        .await
        .unwrap();
    let leader = process.leader();

    tokio::time::sleep(Duration::from_millis(100)).await;
    std::thread::spawn(move || drop(process)).join().unwrap();

    tokio::time::timeout(PROCESS_TIMEOUT, async {
        while ProcessTreeProbe::is_alive(&leader) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("already-exited process must be reaped without a zombie");
}

#[tokio::test]
async fn kill_deadline_includes_a_grandchild_holding_stderr_open() {
    let helper = env!("CARGO_BIN_EXE_ovayra-spike");
    let mut process = GroupedProcess::spawn(helper, &["child-tree", "--hold-stderr"])
        .await
        .unwrap();
    let tree = process
        .wait_for_reported_tree(PROCESS_TIMEOUT)
        .await
        .unwrap();
    let deadline = Duration::from_millis(250);
    let started = std::time::Instant::now();

    let result = process.kill_and_wait(deadline).await;

    assert!(
        started.elapsed() <= deadline.saturating_add(Duration::from_millis(100)),
        "cleanup exceeded its single deadline"
    );
    assert!(
        result.is_ok(),
        "group cleanup must finish before its deadline"
    );
    wait_until_tree_is_dead(&tree).await;
}

async fn wait_until_tree_is_dead(tree: &spike_platform::ProcessTree) {
    tokio::time::timeout(PROCESS_TIMEOUT, async {
        while ProcessTreeProbe::any_alive(tree) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the process group must leave no live parent or grandchild");
}
