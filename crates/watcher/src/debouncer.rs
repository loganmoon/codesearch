//! Event debouncing and aggregation logic
//!
//! This module provides intelligent debouncing of file system events
//! with per-file timers and event aggregation.

#![allow(dead_code)]

use crate::events::{DebouncedEvent, FileChange};
use dashmap::DashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{debug, trace};

/// Manages debouncing and aggregation of file system events
pub struct EventDebouncer {
    /// Debounce window duration
    debounce_duration: Duration,
    /// Map of paths to pending events
    pending_events: Arc<DashMap<PathBuf, DebouncedEvent>>,
    /// Channel to send debounced events
    output_tx: mpsc::Sender<FileChange>,
}

impl EventDebouncer {
    /// Create a new event debouncer
    pub fn new(debounce_duration: Duration, output_tx: mpsc::Sender<FileChange>) -> Self {
        Self {
            debounce_duration,
            pending_events: Arc::new(DashMap::new()),
            output_tx,
        }
    }

    /// Process an incoming event
    pub async fn process_event(&self, event: FileChange) {
        let path = event.path().clone();

        // Update or insert the event - always update to latest
        self.pending_events
            .entry(path.clone())
            .and_modify(|e| {
                trace!("Updating existing event for path: {:?}", path);
                e.update(event.clone());
            })
            .or_insert_with(|| {
                debug!("New event for path: {:?}", path);
                // Schedule debounce timer for new events
                let pending_events = Arc::clone(&self.pending_events);
                let output_tx = self.output_tx.clone();
                let debounce_duration = self.debounce_duration;
                let path_clone = path.clone();

                tokio::spawn(async move {
                    // Wait for debounce window
                    sleep(debounce_duration).await;

                    // Emit the event after debounce period
                    if let Some((_, event)) = pending_events.remove(&path_clone) {
                        debug!(
                            "Emitting debounced event for {:?} (aggregated {} times)",
                            path_clone, event.occurrence_count
                        );
                        let _ = output_tx.send(event.event).await;
                    }
                });

                DebouncedEvent::new(event.clone())
            });
    }

    /// Force flush all pending events
    #[allow(dead_code)]
    pub async fn flush(&self) {
        debug!("Flushing {} pending events", self.pending_events.len());

        let events: Vec<_> = self
            .pending_events
            .iter()
            .map(|entry| entry.value().event.clone())
            .collect();

        self.pending_events.clear();

        for event in events {
            let _ = self.output_tx.send(event).await;
        }
    }

    /// Get the number of pending events
    #[allow(dead_code)]
    pub fn pending_count(&self) -> usize {
        self.pending_events.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::FileMetadata;
    use std::time::SystemTime;

    #[tokio::test]
    async fn test_event_debouncer() {
        let (tx, mut rx) = mpsc::channel(10);
        let debouncer = EventDebouncer::new(Duration::from_millis(50), tx);

        // Send multiple events for the same file
        let path = PathBuf::from("test.rs");
        let event1 = FileChange::Created(
            path.clone(),
            FileMetadata::new(100, SystemTime::now(), 0o644),
        );
        let event2 = FileChange::Modified(
            path.clone(),
            crate::events::DiffStats::new(vec![], vec![], vec![]),
        );

        debouncer.process_event(event1).await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        debouncer.process_event(event2).await;

        // Wait for debounce window
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Should receive the Created event (preserved when followed by Modify)
        let received = rx.try_recv();
        assert!(received.is_ok());
        assert!(matches!(
            received.expect("test setup failed"),
            FileChange::Created(_, _)
        ));

        // Should not receive another event
        assert!(rx.try_recv().is_err());
    }
}
