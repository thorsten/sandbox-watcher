//! Resolve and cache the adapter for the runtime found on this machine.

use std::sync::{Arc, Mutex};

use crate::adapter::RuntimeAdapter;
use crate::detect::{DetectedRuntime, Environment, detect};
use crate::docker::DockerAdapter;
use crate::model::RuntimeKind;
use crate::overview::{DataSource, Overview, RuntimeStatus, load};
use crate::sandbox::TESTED_SANDBOX_VERSION;

/// Holds the connected adapter between refreshes. Reconnects after failures.
#[derive(Default)]
pub struct LiveRuntime {
    adapter: Mutex<Option<Arc<dyn RuntimeAdapter>>>,
}

impl LiveRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    /// Pick the first candidate we have an adapter for.
    pub fn connect(candidates: &[DetectedRuntime]) -> Option<Arc<dyn RuntimeAdapter>> {
        candidates.iter().find_map(|c| match c.kind {
            RuntimeKind::Docker | RuntimeKind::Podman => {
                DockerAdapter::connect(c.kind, &c.endpoint)
                    .ok()
                    .map(|a| Arc::new(a) as Arc<dyn RuntimeAdapter>)
            }
            // Apple `container` adapter arrives in M5.
            RuntimeKind::AppleContainer => None,
        })
    }

    fn cached(&self) -> Option<Arc<dyn RuntimeAdapter>> {
        self.adapter.lock().expect("live adapter poisoned").clone()
    }

    fn set(&self, adapter: Option<Arc<dyn RuntimeAdapter>>) {
        *self.adapter.lock().expect("live adapter poisoned") = adapter;
    }

    pub async fn overview(&self) -> Overview {
        let candidates = detect(&Environment::current());
        if candidates.is_empty() {
            self.set(None);
            return Overview::not_detected(DataSource::Live);
        }
        let adapter = match self.cached().or_else(|| Self::connect(&candidates)) {
            Some(adapter) => adapter,
            None => return Overview::detected_only(candidates),
        };
        let overview = load(adapter.clone(), DataSource::Live, candidates).await;
        match overview.runtime {
            RuntimeStatus::Connected { .. } => self.set(Some(adapter)),
            _ => self.set(None),
        }
        Overview {
            tested_sandbox_version: TESTED_SANDBOX_VERSION.into(),
            ..overview
        }
    }
}
