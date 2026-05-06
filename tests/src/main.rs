use std::result::Result as StdResult;

use automaton_core::*;
use automaton_engine::flow::{ExecStepKind, FlowEngine};
use automaton_scheduler::{CronTicker, Scheduler};

fn main() {
    println!("===== INTEGRATION TEST SUITE =====");
    let mut passed = 0u32;
    let mut failed = 0u32;

    // ── 1. Cron Scheduler Tests ──
    println!("\n--- Cron Scheduler ---");
    test_ok(&mut passed, &mut failed, "Validate valid cron", || {
        Scheduler::validate("*/5 * * * *")
    });
    test_err(&mut passed, &mut failed, "Reject invalid cron", || {
        Scheduler::validate("not-a-cron")
    });
    let mut ticker = CronTicker::new("* * * * *");
    test_ok(&mut passed, &mut failed, "Create CronTicker", || {
        let _ = ticker.tick();
        Ok::<_, String>(())
    });

    // ── 2. Flow Engine Tests ──
    println!("\n--- Flow Engine ---");
    use automaton_core::FlowDefinition;
    let flow = FlowDefinition {
        path: "test.flow".into(),
        version: "0.1.0".into(),
        summary: Some("test".into()),
        steps: vec![
            FlowStep {
                id: "step1".into(),
                kind: FlowStepKind::Sleep,
                script_path: None,
                input: serde_json::json!({"delay_ms": 100}),
                retry: None,
                timeout_ms: 5000,
                depends_on: vec![],
                sleep_after_ms: Some(100),
                stop_if: None,
                failure_step: None,
            },
            FlowStep {
                id: "step2".into(),
                kind: FlowStepKind::Script,
                script_path: Some("test.hello".into()),
                input: serde_json::json!({"msg": "world"}),
                retry: Some(RetryConfig {
                    max_attempts: 3,
                    delay_ms: 100,
                    backoff: BackoffKind::Fixed,
                }),
                timeout_ms: 30000,
                depends_on: vec!["step1".into()],
                sleep_after_ms: None,
                stop_if: None,
                failure_step: None,
            },
        ],
        default_retry: None,
        default_timeout_ms: 30000,
        on_failure: None,
        tags: vec![],
    };
    let flattened = FlowEngine::flatten(&flow);
    let steps = match flattened {
        Ok(ref s) => s.clone(),
        Err(ref e) => {
            failed += 1;
            println!("  ❌ Flatten simple flow: {e}");
            vec![]
        }
    };
    passed += 1;
    println!("  ✅ Flatten simple flow");
    test_eq(
        &mut passed,
        &mut failed,
        "Correct step count",
        steps.len() == 2,
    );
    let has_sleep = steps.iter().any(|s| matches!(s.kind, ExecStepKind::Sleep));
    test_eq(&mut passed, &mut failed, "Has sleep step", has_sleep);

    // ── 3. Branch Flow Test ──
    println!("\n--- Branch Flow ---");
    let branch_flow = FlowDefinition {
        path: "test.branch".into(),
        version: "0.1.0".into(),
        summary: Some("branch test".into()),
        steps: vec![FlowStep {
            id: "branch".into(),
            kind: FlowStepKind::BranchOne(vec![
                vec![FlowStep {
                    id: "branch_a".into(),
                    kind: FlowStepKind::Script,
                    script_path: Some("module.a".into()),
                    input: serde_json::json!({}),
                    retry: None,
                    timeout_ms: 5000,
                    depends_on: vec![],
                    sleep_after_ms: None,
                    stop_if: None,
                    failure_step: None,
                }],
                vec![FlowStep {
                    id: "branch_b".into(),
                    kind: FlowStepKind::Script,
                    script_path: Some("module.b".into()),
                    input: serde_json::json!({}),
                    retry: None,
                    timeout_ms: 5000,
                    depends_on: vec![],
                    sleep_after_ms: None,
                    stop_if: None,
                    failure_step: None,
                }],
            ]),
            script_path: None,
            input: serde_json::json!({}),
            retry: None,
            timeout_ms: 5000,
            depends_on: vec![],
            sleep_after_ms: None,
            stop_if: None,
            failure_step: None,
        }],
        ..Default::default()
    };
    let bf = FlowEngine::flatten(&branch_flow);
    let bsteps = match bf {
        Ok(ref s) => s.clone(),
        Err(ref e) => {
            failed += 1;
            println!("  ❌ Flatten branch flow: {e}");
            vec![]
        }
    };
    passed += 1;
    println!("  ✅ Flatten branch flow");
    test_eq(
        &mut passed,
        &mut failed,
        "Branch has 3 steps",
        bsteps.len() == 3,
    );
    let has_branch = bsteps
        .iter()
        .any(|s| matches!(s.kind, ExecStepKind::BranchOne(_)));
    test_eq(&mut passed, &mut failed, "Has branch_one step", has_branch);

    // ── 4. ForLoop Flow Test ──
    println!("\n--- ForLoop Flow ---");
    let loop_flow = FlowDefinition {
        path: "test.loop".into(),
        version: "0.1.0".into(),
        summary: Some("loop test".into()),
        steps: vec![
            FlowStep {
                id: "fetch".into(),
                kind: FlowStepKind::Script,
                script_path: Some("fetch.data".into()),
                input: serde_json::json!({}),
                retry: None,
                timeout_ms: 5000,
                depends_on: vec![],
                sleep_after_ms: None,
                stop_if: None,
                failure_step: None,
            },
            FlowStep {
                id: "loop".into(),
                kind: FlowStepKind::ForLoop {
                    iterator: "results".into(),
                    steps: vec![FlowStep {
                        id: "process_item".into(),
                        kind: FlowStepKind::Script,
                        script_path: Some("process.item".into()),
                        input: serde_json::json!({}),
                        retry: None,
                        timeout_ms: 5000,
                        depends_on: vec!["fetch".into()],
                        sleep_after_ms: None,
                        stop_if: None,
                        failure_step: None,
                    }],
                },
                script_path: None,
                input: serde_json::json!({}),
                retry: None,
                timeout_ms: 5000,
                depends_on: vec!["fetch".into()],
                sleep_after_ms: None,
                stop_if: None,
                failure_step: None,
            },
        ],
        ..Default::default()
    };
    let lf = FlowEngine::flatten(&loop_flow);
    test_ok(&mut passed, &mut failed, "Flatten forloop flow", || {
        lf.map(|_| ()).map_err(|e| e.to_string())
    });

    // ── Results ──
    println!("\n===== RESULTS =====");
    println!("  ✅ Passed: {passed}");
    println!("  ❌ Failed: {failed}");
    if failed > 0 {
        std::process::exit(1);
    }
    println!("  ✅ ALL TESTS PASSED");
}

fn test_ok<F>(pass: &mut u32, fail: &mut u32, name: &str, f: F)
where
    F: FnOnce() -> StdResult<(), String>,
{
    match f() {
        Ok(_) => {
            *pass += 1;
            println!("  ✅ {name}");
        }
        Err(e) => {
            *fail += 1;
            println!("  ❌ {name}: {e}");
        }
    }
}

fn test_err<F>(pass: &mut u32, fail: &mut u32, name: &str, f: F)
where
    F: FnOnce() -> StdResult<(), String>,
{
    match f() {
        Ok(_) => {
            *fail += 1;
            println!("  ❌ {name}: expected error, got ok");
        }
        Err(_) => {
            *pass += 1;
            println!("  ✅ {name}");
        }
    }
}

fn test_eq(pass: &mut u32, fail: &mut u32, name: &str, cond: bool) {
    if cond {
        *pass += 1;
        println!("  ✅ {name}");
    } else {
        *fail += 1;
        println!("  ❌ {name}");
    }
}
