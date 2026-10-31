//! Behaviour every `RuntimeAdapter` must share. Runs against the mock always,
//! and against a real runtime when `SANDBOX_WATCHER_DOCKER_TESTS=1`.
//!
//! The real-runtime variant expects a container prepared by the caller (see
//! `.github/workflows/ci.yml`): labelled `sandbox.project=<slug>` and
//! `sandbox.hash=...`, running, with `/tmp/.sandbox-ready` present.
//! `SANDBOX_WATCHER_RUNTIME` picks `docker` (default), `podman` or
//! `apple-container`; `DOCKER_HOST` the socket for the first two. With `SANDBOX_WATCHER_SSH_HOST=<alias>`
//! (Unix only) the runtime is reached through that SSH host instead.

use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use sandbox_core::RuntimeAdapter;
use sandbox_core::adapter::{ContainerFilter, ExecRequest, LogOptions};
use sandbox_core::apple::AppleContainerAdapter;
use sandbox_core::docker::DockerAdapter;
use sandbox_core::mock::MockAdapter;
use sandbox_core::model::{ContainerState, Readiness, RuntimeEventKind, RuntimeKind};

async fn wait_until_exited(
    adapter: &Arc<dyn RuntimeAdapter>,
    slug: &str,
    deadline: Duration,
) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < deadline {
        let running = adapter
            .list_containers(&ContainerFilter::for_project(slug))
            .await
            .unwrap_or_default();
        if running.is_empty() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    false
}

async fn conformance(adapter: Arc<dyn RuntimeAdapter>, slug: &str) {
    let caps = adapter.capabilities();
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
    assert_eq!(failed.exit_code, 3);

    if caps.logs {
        adapter
            .logs(
                &container.id,
                LogOptions {
                    tail: Some(5),
                    timestamps: false,
                },
            )
            .await
            .expect("logs");
    }

    if caps.stats {
        let sample = adapter.stats(&container.id).await.expect("stats");
        assert_eq!(sample.container_id, container.id);
    } else {
        assert!(
            adapter.stats(&container.id).await.is_err(),
            "unsupported stats must error"
        );
    }

    let deadline = Duration::from_secs(20);
    if caps.events {
        // Consume the stream before stopping: the request goes out on first
        // poll, and events emitted before that are not replayed.
        let mut events = adapter.events().await.expect("events");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let consumer = tokio::spawn(async move {
            while let Some(item) = events.next().await {
                if tx.send(item).is_err() {
                    break;
                }
            }
        });
        tokio::time::sleep(Duration::from_millis(500)).await;
        adapter
            .stop(&container.id, Some(Duration::from_secs(2)))
            .await
            .expect("stop");

        let mut seen: Vec<String> = Vec::new();
        let outcome: Result<bool, String> = tokio::time::timeout(deadline, async {
            while let Some(item) = rx.recv().await {
                let event = match item {
                    Ok(event) => event,
                    Err(e) => {
                        seen.push(format!("error: {e}"));
                        return Err(e.to_string());
                    }
                };
                seen.push(format!(
                    "{} {:?} {}",
                    event.container_id.chars().take(12).collect::<String>(),
                    event.kind,
                    event.container_name.as_deref().unwrap_or("")
                ));
                if event.container_id == container.id
                    && matches!(
                        event.kind,
                        RuntimeEventKind::Stopped | RuntimeEventKind::Died
                    )
                {
                    return Ok(true);
                }
            }
            Ok(false)
        })
        .await
        .unwrap_or(Ok(false));
        consumer.abort();
        match outcome {
            Ok(saw_stop) => assert!(
                saw_stop,
                "stop or die event for {} within {deadline:?}; events seen: {seen:?}",
                container.name
            ),
            // Some setups (rootless Podman without journald) have no events
            // backend. The app falls back to polling there; so does this test.
            Err(e) => eprintln!("WARNING: event stream unavailable, falling back to polling: {e}"),
        }
    } else {
        adapter
            .stop(&container.id, Some(Duration::from_secs(2)))
            .await
            .expect("stop");
    }
    assert!(
        wait_until_exited(&adapter, slug, deadline).await,
        "container stopped within {deadline:?}"
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
async fn real_runtime_adapter_conforms() {
    if std::env::var("SANDBOX_WATCHER_DOCKER_TESTS")
        .ok()
        .as_deref()
        != Some("1")
    {
        eprintln!("skipped: set SANDBOX_WATCHER_DOCKER_TESTS=1 with a prepared container");
        return;
    }
    let slug = std::env::var("SANDBOX_WATCHER_TEST_SLUG").unwrap_or_else(|_| "watcher-test".into());
    let runtime = std::env::var("SANDBOX_WATCHER_RUNTIME").unwrap_or_else(|_| "docker".into());
    #[cfg(unix)]
    if let Ok(alias) = std::env::var("SANDBOX_WATCHER_SSH_HOST") {
        let (session, adapters, _) =
            sandbox_core::remote::connect_host(&alias, Duration::from_secs(20))
                .await
                .expect("connect over ssh");
        let hosted = adapters
            .into_iter()
            .find(|a| a.adapter.kind().as_str() == runtime)
            .expect("runtime detected over ssh");
        conformance(hosted.adapter.clone(), &slug).await;

        // Reconnect after the master goes away.
        session.close().await;
        assert!(!session.check().await, "master is gone after close");
        let (again, adapters, _) =
            sandbox_core::remote::connect_host(&alias, Duration::from_secs(20))
                .await
                .expect("reconnect over ssh");
        assert!(!adapters.is_empty());
        again.close().await;
        return;
    }
    let adapter: Arc<dyn RuntimeAdapter> = match runtime.as_str() {
        "apple-container" => Arc::new(AppleContainerAdapter::new("container")),
        "podman" | "docker" => {
            let kind = if runtime == "podman" {
                RuntimeKind::Podman
            } else {
                RuntimeKind::Docker
            };
            let endpoint =
                std::env::var("DOCKER_HOST").unwrap_or_else(|_| "/var/run/docker.sock".into());
            Arc::new(DockerAdapter::connect(kind, &endpoint).expect("connect"))
        }
        other => panic!("unknown SANDBOX_WATCHER_RUNTIME {other}"),
    };
    conformance(adapter, &slug).await;
}
