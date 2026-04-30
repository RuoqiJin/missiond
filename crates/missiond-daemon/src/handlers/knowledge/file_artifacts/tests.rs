use super::*;

// ── topic sanitize ────────────────────────────────────────────────────

#[test]
fn sanitize_topic_keeps_safe_chars() {
    assert_eq!(sanitize_topic_segment("foo"), "foo");
    assert_eq!(sanitize_topic_segment("foo_bar-baz"), "foo_bar-baz");
    assert_eq!(sanitize_topic_segment("alignment-v2"), "alignment-v2");
}

#[test]
fn sanitize_topic_collapses_unsafe_runs_into_single_hyphen() {
    assert_eq!(sanitize_topic_segment("Foo Bar/Baz!"), "Foo-Bar-Baz");
    assert_eq!(sanitize_topic_segment("hello   world"), "hello-world");
    assert_eq!(sanitize_topic_segment("a/b/c"), "a-b-c");
}

#[test]
fn sanitize_topic_falls_back_for_empty_or_pure_separators() {
    assert_eq!(sanitize_topic_segment(""), ANONYMOUS_TOPIC);
    assert_eq!(sanitize_topic_segment("///"), ANONYMOUS_TOPIC);
    assert_eq!(sanitize_topic_segment("---"), ANONYMOUS_TOPIC);
    assert_eq!(sanitize_topic_segment("   "), ANONYMOUS_TOPIC);
}

#[test]
fn sanitize_topic_is_idempotent() {
    for raw in &["", "Foo Bar/Baz", "alignment-v2", "///", "a  b  c"] {
        let once = sanitize_topic_segment(raw);
        let twice = sanitize_topic_segment(&once);
        assert_eq!(once, twice, "sanitize must be idempotent for {:?}", raw);
    }
}

// ── artifact_path ─────────────────────────────────────────────────────

#[test]
fn artifact_path_alignment_lives_under_alignment_topic_dir() {
    let p = artifact_path(
        Path::new("/tmp/proj"),
        ArtifactKind::IntentAlignment,
        "wave11-foo",
    );
    assert_eq!(
        p,
        PathBuf::from("/tmp/proj/.missiond/alignment/wave11-foo/intent-alignment.lisp")
    );
}

#[test]
fn artifact_path_plan_lives_under_plans_topic_dir() {
    let p = artifact_path(Path::new("/tmp/proj"), ArtifactKind::Plan, "wave11-foo");
    assert_eq!(
        p,
        PathBuf::from("/tmp/proj/.missiond/plans/wave11-foo/PLAN.lisp")
    );
}

#[test]
fn artifact_path_workflow_lives_under_workflows_dir_with_topic_filename() {
    let p = artifact_path(Path::new("/tmp/proj"), ArtifactKind::Workflow, "wave11-foo");
    assert_eq!(
        p,
        PathBuf::from("/tmp/proj/.missiond/workflows/wave11-foo.lisp")
    );
}

#[test]
fn artifact_path_sanitizes_topic_internally() {
    let p = artifact_path(
        Path::new("/tmp/proj"),
        ArtifactKind::IntentAlignment,
        "Foo Bar/Baz!",
    );
    assert_eq!(
        p,
        PathBuf::from("/tmp/proj/.missiond/alignment/Foo-Bar-Baz/intent-alignment.lisp")
    );
}

#[test]
fn artifact_path_uses_anonymous_fallback_for_empty_topic() {
    let p = artifact_path(Path::new("/tmp/proj"), ArtifactKind::Plan, "");
    assert_eq!(
        p,
        PathBuf::from("/tmp/proj/.missiond/plans/anonymous/PLAN.lisp")
    );
}

#[test]
fn artifact_path_from_spec_honors_file_name_override() {
    let spec = ArtifactSpec {
        kind: ArtifactKind::IntentAlignment,
        topic: "wave11-foo".to_string(),
        project_root: PathBuf::from("/tmp/proj"),
        file_name: Some("intent-alignment-v2.lisp".to_string()),
    };
    let p = artifact_path_from_spec(&spec);
    assert_eq!(
        p,
        PathBuf::from("/tmp/proj/.missiond/alignment/wave11-foo/intent-alignment-v2.lisp")
    );
}

#[test]
fn artifact_path_from_spec_falls_through_when_override_empty() {
    let spec = ArtifactSpec {
        kind: ArtifactKind::Plan,
        topic: "wave11-foo".to_string(),
        project_root: PathBuf::from("/tmp/proj"),
        file_name: Some(String::new()),
    };
    let p = artifact_path_from_spec(&spec);
    assert_eq!(
        p,
        PathBuf::from("/tmp/proj/.missiond/plans/wave11-foo/PLAN.lisp")
    );
}

// ── atomic_write_artifact ─────────────────────────────────────────────

#[test]
fn atomic_write_creates_parent_directories() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let target = artifact_path(tmp.path(), ArtifactKind::Plan, "wave11-foo");
    assert!(
        !target.parent().unwrap().exists(),
        "precondition: no parent yet"
    );

    let outcome = atomic_write_artifact(&target, "(plan :id foo)\n", false)
        .expect("first write should succeed");
    assert!(outcome.created);
    assert!(!outcome.overwritten);
    assert!(target.exists());
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "(plan :id foo)\n".to_string()
    );
}

#[test]
fn atomic_write_refuses_overwrite_by_default() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let target = artifact_path(tmp.path(), ArtifactKind::Workflow, "wave11-foo");

    atomic_write_artifact(&target, "first", false).expect("seed write");

    let err = atomic_write_artifact(&target, "second", false)
        .expect_err("must refuse overwrite without flag");
    let msg = format!("{}", err);
    assert!(
        msg.contains("already exists"),
        "error must explain refusal, got: {}",
        msg
    );
    // File contents unchanged.
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "first");
}

#[test]
fn atomic_write_replaces_when_overwrite_true() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let target = artifact_path(tmp.path(), ArtifactKind::IntentAlignment, "wave11-foo");

    let first = atomic_write_artifact(&target, "v1", false).expect("seed write");
    assert!(first.created);
    assert!(!first.overwritten);

    let second = atomic_write_artifact(&target, "v2", true).expect("overwrite write");
    assert!(!second.created);
    assert!(second.overwritten);
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "v2");
}

/// Computed at runtime so the source text never contains the literal
/// fixed-extension form we banned. The grep self-check therefore stays
/// 0-hit on the production source while the assertion still detects a
/// regression that re-creates the pre-wave-11 temp file.
fn legacy_fixed_temp_path(target: &Path) -> PathBuf {
    let banned_ext = format!("{}.{}", "tmp", "write");
    target.with_extension(banned_ext)
}

#[test]
fn atomic_write_does_not_leave_temp_file_after_success() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let target = artifact_path(tmp.path(), ArtifactKind::Plan, "wave11-foo");
    atomic_write_artifact(&target, "data", false).expect("write");
    // No `.tmp.*` siblings should remain in the target's directory.
    // Specifically, the legacy fixed-extension form must never reappear
    // (pairs with `unique_temp_path_in_dir_does_not_collide_back_to_legacy_form`).
    let parent = target.parent().expect("plan artifact has parent");
    let leftovers: Vec<_> = std::fs::read_dir(parent)
        .expect("read parent dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().contains(".tmp."))
        .collect();
    assert!(
        leftovers.is_empty(),
        "no temp siblings should remain after success, found: {:?}",
        leftovers.iter().map(|e| e.file_name()).collect::<Vec<_>>()
    );
    let legacy = legacy_fixed_temp_path(&target);
    assert!(
        !legacy.exists(),
        "must not regress to the legacy fixed-extension form: {}",
        legacy.display()
    );
}

// ── unique_temp_path_in_dir ──────────────────────────────────────────

#[test]
fn unique_temp_path_in_dir_keeps_target_directory() {
    let target = PathBuf::from("/tmp/proj/.missiond/plans/wave11-foo/PLAN.lisp");
    let tmp = unique_temp_path_in_dir(&target);
    assert_eq!(
        tmp.parent(),
        target.parent(),
        "temp path must live in the target's directory so `rename` is same-FS atomic"
    );
}

#[test]
fn unique_temp_path_in_dir_two_calls_produce_distinct_paths() {
    let target = PathBuf::from("/tmp/proj/.missiond/plans/wave11-foo/PLAN.lisp");
    let a = unique_temp_path_in_dir(&target);
    let b = unique_temp_path_in_dir(&target);
    assert_ne!(
        a, b,
        "back-to-back calls must yield distinct temp paths (counter disambiguates same-nanos collisions)"
    );
}

#[test]
fn unique_temp_path_in_dir_does_not_collide_back_to_legacy_form() {
    // The bug we are guarding against: regressing to the fixed
    // `with_extension("…")` form that two concurrent writers would
    // share. The unique helper's leaf must contain the original leaf
    // plus a `.tmp.<pid>.<nanos>.<seq>` suffix.
    let target = PathBuf::from("/tmp/proj/.missiond/plans/wave11-foo/PLAN.lisp");
    let tmp = unique_temp_path_in_dir(&target);
    let leaf = tmp
        .file_name()
        .expect("temp has leaf")
        .to_string_lossy()
        .into_owned();
    assert!(
        leaf.starts_with("PLAN.lisp.tmp."),
        "temp leaf should keep target leaf as prefix, got: {}",
        leaf
    );
    // Constructed via runtime concat so the source text doesn't contain
    // the banned literal — the production source therefore stays clean
    // under the wave-11 self-check rg pattern.
    let banned_suffix = format!(".{}.{}", "tmp", "write");
    assert!(
        !leaf.ends_with(&banned_suffix),
        "must not regress to the legacy fixed-extension form: {}",
        leaf
    );
    let legacy = legacy_fixed_temp_path(&target);
    assert_ne!(
        tmp, legacy,
        "unique helper must not produce the legacy fixed-extension path"
    );
}

#[test]
fn unique_temp_path_in_dir_handles_target_without_leaf() {
    // Defensive: a path with no leaf shouldn't panic; we synthesize an
    // `anonymous` placeholder so the rename still has a same-dir target.
    let target = PathBuf::from("/");
    let tmp = unique_temp_path_in_dir(&target);
    let leaf = tmp
        .file_name()
        .expect("temp has leaf")
        .to_string_lossy()
        .into_owned();
    assert!(leaf.starts_with("anonymous.tmp."), "got: {}", leaf);
}

#[test]
fn concurrent_writes_to_distinct_artifacts_all_succeed() {
    // Exercises the unique-temp helper end-to-end under thread contention.
    // 8 threads each write a different artifact under the same tempdir;
    // none should fail, none should leak temp files.
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().to_path_buf();
    let n = 8usize;
    let handles: Vec<_> = (0..n)
        .map(|i| {
            let root = root.clone();
            std::thread::spawn(move || {
                let topic = format!("concurrent-{}", i);
                let target = artifact_path(&root, ArtifactKind::Plan, &topic);
                let body = format!("(plan :id {})\n", i);
                let outcome = atomic_write_artifact(&target, &body, false)
                    .expect("concurrent write should succeed");
                assert!(outcome.created);
                assert_eq!(std::fs::read_to_string(&target).unwrap(), body);
            })
        })
        .collect();
    for h in handles {
        h.join().expect("worker thread panicked");
    }
    // Walk every generated parent dir; assert no temp leftovers.
    for i in 0..n {
        let topic = format!("concurrent-{}", i);
        let target = artifact_path(&root, ArtifactKind::Plan, &topic);
        let parent = target.parent().unwrap();
        let leftovers: Vec<_> = std::fs::read_dir(parent)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp."))
            .collect();
        assert!(
            leftovers.is_empty(),
            "topic {} leaked temp files: {:?}",
            topic,
            leftovers.iter().map(|e| e.file_name()).collect::<Vec<_>>()
        );
    }
}

#[test]
fn atomic_write_returns_correct_sha256_and_bytes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let target = artifact_path(tmp.path(), ArtifactKind::Workflow, "wave11-foo");
    let content = "(workflow :id wave11-foo)\n";
    let outcome = atomic_write_artifact(&target, content, false).expect("write");

    // Reference SHA-256 computed independently below.
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let mut expected = String::new();
    for b in hasher.finalize() {
        use std::fmt::Write as _;
        let _ = write!(&mut expected, "{:02x}", b);
    }
    assert_eq!(outcome.sha256, expected);
    assert_eq!(outcome.bytes, content.as_bytes().len() as u64);
}

// ── read_existing_metadata ───────────────────────────────────────────

#[test]
fn read_existing_metadata_returns_none_when_absent() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let target = artifact_path(tmp.path(), ArtifactKind::Plan, "missing");
    let result = read_existing_metadata(&target).expect("ok with None");
    assert!(result.is_none());
}

#[test]
fn read_existing_metadata_matches_write_outcome() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let target = artifact_path(tmp.path(), ArtifactKind::IntentAlignment, "wave11-foo");
    let content = "(intent-alignment :id foo)\n";
    let outcome = atomic_write_artifact(&target, content, false).expect("write");

    let meta = read_existing_metadata(&target)
        .expect("read")
        .expect("file must exist after write");
    assert_eq!(meta.path, target);
    assert_eq!(meta.sha256, outcome.sha256);
    assert_eq!(meta.bytes, outcome.bytes);
}

// ── ArtifactKind label ────────────────────────────────────────────────

#[test]
fn artifact_kind_labels_are_stable() {
    assert_eq!(ArtifactKind::IntentAlignment.label(), "intent-alignment");
    assert_eq!(ArtifactKind::Plan.label(), "plan");
    assert_eq!(ArtifactKind::Workflow.label(), "workflow");
    assert_eq!(format!("{}", ArtifactKind::Plan), "plan");
}

// ── attempt_artifact_write / WriterContext ───────────────────────────
//
// wave-14 :: writer integration. Pin the resolver contract,
// overwrite policy, and the JSON splice shape so directive / plan /
// workflow callers all see the same `file_*` keys + partial-status
// semantics. We exercise the resolver branch via a real
// `SharedProjectRegistry` (no AppState graph required) and the write
// branch through `tempfile`.

use missiond_core::types::{ProjectConfig, ProjectRegistry};
use std::sync::Arc;
use tokio::sync::RwLock;

fn registry_with(projects: Vec<ProjectConfig>) -> SharedProjectRegistry {
    Arc::new(RwLock::new(ProjectRegistry::new(projects)))
}

fn project(id: &str, path: &str) -> ProjectConfig {
    ProjectConfig {
        id: id.to_string(),
        path: path.to_string(),
        intent_path: None,
        active: true,
        slots: vec![],
        github_url: None,
        kind: "managed".to_string(),
        vault_path: None,
        parent_id: None,
        created_at: None,
        updated_at: None,
    }
}

#[tokio::test]
async fn attempt_writes_alignment_under_registered_project_root() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().unwrap();
    let reg = registry_with(vec![project("missiond", &root.display().to_string())]);

    let outcome = attempt_artifact_write(
        &reg,
        WriterContext {
            kind: ArtifactKind::IntentAlignment,
            topic: "wave14-task-01",
            project: Some("missiond"),
            cwd: None,
            target_project: None,
            overwrite: false,
        },
        "(intent-alignment :id wave14-task-01)\n",
    )
    .await;

    match outcome {
        AttemptOutcome::Written(w) => {
            assert!(w.created);
            assert!(!w.overwritten);
            let expected = root
                .join(".missiond")
                .join("alignment")
                .join("wave14-task-01")
                .join("intent-alignment.lisp");
            assert_eq!(w.path, expected);
            assert!(expected.exists());
        }
        other => panic!("expected Written, got {:?}", other),
    }
}

#[tokio::test]
async fn attempt_refuses_overwrite_by_default_and_surfaces_partial() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().unwrap();
    let reg = registry_with(vec![project("missiond", &root.display().to_string())]);

    let _seed = attempt_artifact_write(
        &reg,
        WriterContext {
            kind: ArtifactKind::Plan,
            topic: "wave14-foo",
            project: Some("missiond"),
            cwd: None,
            target_project: None,
            overwrite: false,
        },
        "v1",
    )
    .await;

    let second = attempt_artifact_write(
        &reg,
        WriterContext {
            kind: ArtifactKind::Plan,
            topic: "wave14-foo",
            project: Some("missiond"),
            cwd: None,
            target_project: None,
            overwrite: false,
        },
        "v2",
    )
    .await;

    match second {
        AttemptOutcome::WriteFailed { reason, path } => {
            assert!(
                reason.contains("already exists"),
                "expected overwrite refusal, got {}",
                reason
            );
            // Splicing into a fresh `compiled` payload should downgrade to
            // partial and stamp file_path / file_write_error.
            let mut payload = json!({"status": "compiled", "plan_id": "abc"});
            AttemptOutcome::WriteFailed {
                path: path.clone(),
                reason: reason.clone(),
            }
            .splice_into(&mut payload);
            assert_eq!(payload["status"], "partial");
            assert_eq!(payload["plan_id"], "abc", "DB-row fields preserved");
            assert_eq!(payload["file_written"], false);
            assert_eq!(payload["file_path"], path.display().to_string());
            assert!(payload["file_write_error"]
                .as_str()
                .unwrap()
                .contains("already exists"));
        }
        other => panic!("expected WriteFailed, got {:?}", other),
    }
}

#[tokio::test]
async fn attempt_replaces_when_overwrite_true() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().unwrap();
    let reg = registry_with(vec![project("missiond", &root.display().to_string())]);

    let _seed = attempt_artifact_write(
        &reg,
        WriterContext {
            kind: ArtifactKind::Workflow,
            topic: "wave14-foo",
            project: Some("missiond"),
            cwd: None,
            target_project: None,
            overwrite: false,
        },
        "v1",
    )
    .await;
    let second = attempt_artifact_write(
        &reg,
        WriterContext {
            kind: ArtifactKind::Workflow,
            topic: "wave14-foo",
            project: Some("missiond"),
            cwd: None,
            target_project: None,
            overwrite: true,
        },
        "v2",
    )
    .await;
    match second {
        AttemptOutcome::Written(w) => {
            assert!(!w.created);
            assert!(w.overwritten);
            let expected = root
                .join(".missiond")
                .join("workflows")
                .join("wave14-foo.lisp");
            assert_eq!(std::fs::read_to_string(&expected).unwrap(), "v2");
        }
        other => panic!("expected Written, got {:?}", other),
    }
}

#[tokio::test]
async fn attempt_rejects_relative_cwd_no_process_cwd_fallback() {
    let reg = registry_with(vec![project("missiond", "/tmp/missiond")]);
    let outcome = attempt_artifact_write(
        &reg,
        WriterContext {
            kind: ArtifactKind::Plan,
            topic: "wave14-foo",
            project: None,
            cwd: Some("relative/path"),
            target_project: None,
            overwrite: false,
        },
        "data",
    )
    .await;
    match outcome {
        AttemptOutcome::ResolveFailed { reason } => {
            assert!(
                reason.contains("not absolute"),
                "expected absolute-cwd refusal, got {}",
                reason
            );
            assert!(
                reason.contains("project-root-spawn-cwd"),
                "expected lisp contract reference, got {}",
                reason
            );
        }
        other => panic!("expected ResolveFailed, got {:?}", other),
    }
}

#[tokio::test]
async fn attempt_rejects_no_signal_no_process_cwd_fallback() {
    let reg = registry_with(vec![project("missiond", "/tmp/missiond")]);
    let outcome = attempt_artifact_write(
        &reg,
        WriterContext {
            kind: ArtifactKind::Plan,
            topic: "wave14-foo",
            project: None,
            cwd: None,
            target_project: None,
            overwrite: false,
        },
        "data",
    )
    .await;
    match outcome {
        AttemptOutcome::ResolveFailed { reason } => {
            assert!(
                reason.contains("no project_id")
                    || reason.contains("no signal")
                    || reason.contains("absolute cwd"),
                "expected fail-fast on missing signal, got {}",
                reason
            );
        }
        other => panic!("expected ResolveFailed, got {:?}", other),
    }
}

#[tokio::test]
async fn attempt_uses_target_project_fallback() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().unwrap();
    let reg = registry_with(vec![project("missiond", &root.display().to_string())]);

    let outcome = attempt_artifact_write(
        &reg,
        WriterContext {
            kind: ArtifactKind::Plan,
            topic: "wave14-foo",
            project: None,
            cwd: None,
            target_project: Some("missiond"),
            overwrite: false,
        },
        "(plan)\n",
    )
    .await;
    let written = match outcome {
        AttemptOutcome::Written(w) => w,
        other => panic!("expected Written, got {:?}", other),
    };
    let expected = root
        .join(".missiond")
        .join("plans")
        .join("wave14-foo")
        .join("PLAN.lisp");
    assert_eq!(written.path, expected);
}

#[tokio::test]
async fn attempt_explicit_project_wins_over_target_project_fallback() {
    let tmp_a = tempfile::tempdir().expect("tempdir");
    let tmp_b = tempfile::tempdir().expect("tempdir");
    let root_a = tmp_a.path().canonicalize().unwrap();
    let root_b = tmp_b.path().canonicalize().unwrap();
    let reg = registry_with(vec![
        project("alpha", &root_a.display().to_string()),
        project("beta", &root_b.display().to_string()),
    ]);
    let outcome = attempt_artifact_write(
        &reg,
        WriterContext {
            kind: ArtifactKind::IntentAlignment,
            topic: "shared",
            project: Some("alpha"),
            cwd: None,
            target_project: Some("beta"),
            overwrite: false,
        },
        "(intent-alignment :id shared)",
    )
    .await;
    let w = match outcome {
        AttemptOutcome::Written(w) => w,
        other => panic!("expected Written, got {:?}", other),
    };
    assert!(
        w.path.starts_with(&root_a),
        "explicit project=alpha must win, but wrote to {:?}",
        w.path
    );
}

#[tokio::test]
async fn attempt_resolves_via_absolute_cwd_under_project_root() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().unwrap();
    let subdir = root.join("crates").join("missiond-daemon");
    std::fs::create_dir_all(&subdir).unwrap();
    let reg = registry_with(vec![project("missiond", &root.display().to_string())]);

    let outcome = attempt_artifact_write(
        &reg,
        WriterContext {
            kind: ArtifactKind::IntentAlignment,
            topic: "wave14-foo",
            project: None,
            cwd: Some(subdir.display().to_string().as_str()),
            target_project: None,
            overwrite: false,
        },
        "(intent-alignment)",
    )
    .await;
    let w = match outcome {
        AttemptOutcome::Written(w) => w,
        other => panic!("expected Written, got {:?}", other),
    };
    // longest-prefix collapse — file lands under root, NOT subdir.
    assert!(w.path.starts_with(&root));
    assert!(!w.path.starts_with(&subdir));
}

// ── splice_into shape ────────────────────────────────────────────────

#[test]
fn splice_writes_emits_canonical_keys() {
    let mut payload = json!({"status": "compiled", "directive_id": "abc"});
    let outcome = AttemptOutcome::Written(WriteOutcome {
        path: PathBuf::from("/tmp/x/intent-alignment.lisp"),
        created: true,
        overwritten: false,
        sha256: "deadbeef".to_string(),
        bytes: 12,
    });
    outcome.splice_into(&mut payload);
    assert_eq!(
        payload["status"], "compiled",
        "Written must NOT downgrade status"
    );
    assert_eq!(payload["directive_id"], "abc");
    assert_eq!(payload["file_written"], true);
    assert_eq!(payload["file_path"], "/tmp/x/intent-alignment.lisp");
    assert_eq!(payload["file_sha256"], "deadbeef");
    assert_eq!(payload["file_bytes"], 12);
    assert_eq!(payload["file_created"], true);
    assert_eq!(payload["file_overwritten"], false);
}

#[test]
fn splice_resolve_failed_downgrades_status_and_includes_error() {
    let mut payload = json!({"status": "compiled", "plan_id": "p1"});
    let outcome = AttemptOutcome::ResolveFailed {
        reason: "no project_id supplied".to_string(),
    };
    outcome.splice_into(&mut payload);
    assert_eq!(payload["status"], "partial");
    assert_eq!(payload["plan_id"], "p1");
    assert_eq!(payload["file_written"], false);
    assert!(payload.get("file_path").is_none());
    assert_eq!(payload["file_write_error"], "no project_id supplied");
}

#[test]
fn splice_write_failed_keeps_already_partial_status() {
    let mut payload = json!({"status": "partial", "note": "set earlier"});
    let outcome = AttemptOutcome::WriteFailed {
        path: PathBuf::from("/tmp/x/PLAN.lisp"),
        reason: "permission denied".to_string(),
    };
    outcome.splice_into(&mut payload);
    // Already partial; we don't double-stamp.
    assert_eq!(payload["status"], "partial");
    assert_eq!(payload["note"], "set earlier");
    assert_eq!(payload["file_written"], false);
    assert_eq!(payload["file_path"], "/tmp/x/PLAN.lisp");
    assert_eq!(payload["file_write_error"], "permission denied");
}
