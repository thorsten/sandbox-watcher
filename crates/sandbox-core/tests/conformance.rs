//! Behaviour every `RuntimeAdapter` must share. Runs against the mock always,
//! and against a real Docker daemon when `SANDBOX_WATCHER_DOCKER_TESTS=1`.
//!
//! The Docker variant expects a container prepared by the caller (see
//! `.github/workflows/ci.yml`): labelled `sandbox.project=<slug>` and
//! `sandbox.hash=...`, running, with `/tmp/.sandbox-ready` present.

use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use sandbox_core::RuntimeAdapter;
use sandbox_core::adapter::{ContainerFilter, ExecRequest, LogOptions};
use sandbox_core::docker::DockerAdapter;
use sandbox_core::mock::MockAdapter;
use sandbox_core::model::{ContainerState, Readiness, RuntimeEventKind, RuntimeKind};

async fn conformance(adapter: Arc<dyn RuntimeAdapter>, slug: &str) {
    let info = adapter.ping().await.expect("ping");
    assert_eq!(info.kind, adapter.kind());

    let all = adapter
        .list_containers(&ContainerFilter::all())
        .await
        .expect("list all");
    assert!(
        all.iter().all(|c| c.name.starts_with("sandbox-")),
        "only sandbox containers"
    );

    let mine = adapter
        .list_containers(&ContainerFilter::for_project(slug))
        .await
        .expect("list project");
    assert_eq!(
        mine.len(),
        1,
        "exactly one running container for {slug}: {mine:?}"
    );
    let container = &mine[0];
    assert_eq!(container.project_slug, slug);
    assert_eq!(container.state, ContainerState::Running);
    assert!(container.config_hash.is_some(), "sandbox.hash label mapped");

    let details = adapter
        .inspect(&container.id)
        .await
        .expect("inspect")
        .expect("exists");
    assert_eq!(details.container.id, container.id);
    assert!(
        details.container.started_at.is_some(),
        "started_at from inspect"
    );
    assert_eq!(details.readiness, Readiness::Ready);
    assert!(details.env_names.iter().all(|n| !n.contains('=')));
    assert!(
        adapter
            .inspect("does-not-exist-sandbox-watcher")
            .await
            .expect("inspect missing")
            .is_none()
    );

    let exec = adapter
        .exec(
            &container.id,
            ExecRequest::new(vec!["sh".into(), "-c".into(), "echo hello".into()]),
        )
        .await
        .expect("exec");
    assert_eq!(exec.exit_code, 0);
    assert!(!exec.stdout.trim().is_empty());

    let failed = adapter
        .exec(
            &container.id,
            ExecRequest::new(vec!["sh".into(), "-c".into(), "exit 3".into()]),
        )
        .await
        .expect("exec with non-zero exit");
    assert!(failed.exit_code == 3 || adapter.kind() != RuntimeKind::Docker);

    let logs = adapter
        .logs(
            &container.id,
            LogOptions {
                tail: Some(5),
                timestamps: false,
            },
        )
        .await;
    assert!(logs.is_ok());

    let sample = adapter.stats(&container.id).await.expect("stats");
    assert_eq!(sample.container_id, container.id);

    let mut events = adapter.events().await.expect("events");
    adapter
        .stop(&container.id, Some(Duration::from_secs(2)))
        .await
        .expect("stop");

    let deadline = Duration::from_secs(20);
    let saw_stop = tokio::time::timeout(deadline, async {
        while let Some(event) = events.next().await {
            let event = event.expect("event");
            if event.container_id == container.id
                && matches!(
                    event.kind,
                    RuntimeEventKind::Stopped | RuntimeEventKind::Died
                )
            {
                return true;
            }
        }
        false
    })
    .await
    .unwrap_or(false);
    assert!(
        saw_stop,
        "stop or die event for {} within {deadline:?}",
        container.name
    );

    let still_running = adapter
        .list_containers(&ContainerFilter::for_project(slug))
        .await
        .expect("list after stop");
    assert!(
        still_running.is_empty(),
        "no running container after stop: {still_running:?}"
    );
    let including_stopped = adapter
        .list_containers(&ContainerFilter {
            project_slug: Some(slug.into()),
            include_stopped: true,
            states: vec![],
        })
        .await
        .expect("list stopped");
    assert_eq!(including_stopped.len(), 1);
    assert_eq!(including_stopped[0].state, ContainerState::Exited);

    adapter
        .stop(&container.id, None)
        .await
        .expect("stopping twice is fine");
}

#[tokio::test]
async fn mock_adapter_conforms() {
    conformance(Arc::new(MockAdapter::demo()), "billing-api-live").await;
}

#[tokio::test]
async fn docker_adapter_conforms() {
    if std::env::var("SANDBOX_WATCHER_DOCKER_TESTS")
        .ok()
        .as_deref()
        != Some("1")
    {
        eprintln!("skipped: set SANDBOX_WATCHER_DOCKER_TESTS=1 with a prepared container");
        return;
    }
    let slug = std::env::var("SANDBOX_WATCHER_TEST_SLUG").unwrap_or_else(|_| "watcher-test".into());
    let endpoint = std::env::var("DOCKER_HOST").unwrap_or_else(|_| "/var/run/docker.sock".into());
    let adapter = DockerAdapter::connect(RuntimeKind::Docker, &endpoint).expect("connect");
    conformance(Arc::new(adapter), &slug).await;
}
