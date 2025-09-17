//! Event debouncing and aggregation logic
//!
//! This module provides intelligent debouncing of file system events
//! with per-file timers and event aggregation.

use crate::events::{DebouncedEvent, FileChange};
use dashmap::DashMap;
use std::collections::BinaryHeap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::mpsc;
use tokio::time::{interval, sleep};
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
    pub fn pending_count(&self) -> usize {
        self.pending_events.len()
    }
}

/// Aggregates multiple events for the same file
pub struct EventAggregator {
    /// Priority queue for event ordering
    event_queue: BinaryHeap<PrioritizedEvent>,
    /// Maximum events to aggregate
    max_batch_size: usize,
}

impl EventAggregator {
    /// Create a new event aggregator
    pub fn new(max_batch_size: usize) -> Self {
        Self {
            event_queue: BinaryHeap::new(),
            max_batch_size,
        }
    }

    /// Add an event to the aggregator
    pub fn add_event(&mut self, event: FileChange) {
        let priority = event.priority();
        let prioritized = PrioritizedEvent {
            priority,
            timestamp: SystemTime::now(),
            event,
        };
        self.event_queue.push(prioritized);
    }

    /// Aggregate events for the same path
    pub fn aggregate_events(&mut self) -> Vec<FileChange> {
        let mut aggregated = Vec::new();
        let mut seen_paths = std::collections::HashSet::new();
        let mut temp_queue = Vec::new();

        // Process events by priority
        while let Some(prioritized) = self.event_queue.pop() {
            let path = prioritized.event.path().clone();

            if seen_paths.contains(&path) {
                // Skip duplicate paths, keeping higher priority event
                temp_queue.push(prioritized);
            } else {
                seen_paths.insert(path);
                aggregated.push(prioritized.event);

                if aggregated.len() >= self.max_batch_size {
                    break;
                }
            }
        }

        // Re-insert unprocessed events
        for event in temp_queue {
            self.event_queue.push(event);
        }

        aggregated
    }

    /// Check if aggregator is empty
    pub fn is_empty(&self) -> bool {
        self.event_queue.is_empty()
    }

    /// Get the number of queued events
    pub fn len(&self) -> usize {
        self.event_queue.len()
    }

    /// Clear all events
    pub fn clear(&mut self) {
        self.event_queue.clear();
    }
}

/// Event with priority for ordering
#[derive(Debug, Clone)]
struct PrioritizedEvent {
    priority: u8,
    timestamp: SystemTime,
    event: FileChange,
}

impl PartialEq for PrioritizedEvent {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority && self.timestamp == other.timestamp
    }
}

impl Eq for PrioritizedEvent {}

impl PartialOrd for PrioritizedEvent {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PrioritizedEvent {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Lower priority value = higher priority (reverse order for max heap)
        other
            .priority
            .cmp(&self.priority)
            .then_with(|| other.timestamp.cmp(&self.timestamp))
    }
}

/// Manages batch processing of events
pub struct BatchProcessor {
    /// Event aggregator
    aggregator: EventAggregator,
    /// Maximum batch size
    batch_size: usize,
    /// Batch timeout duration
    batch_timeout: Duration,
    /// Channel to receive events
    input_rx: mpsc::Receiver<FileChange>,
    /// Channel to send batches
    output_tx: mpsc::Sender<Vec<FileChange>>,
}

impl BatchProcessor {
    /// Create a new batch processor
    pub fn new(
        batch_size: usize,
        batch_timeout: Duration,
        input_rx: mpsc::Receiver<FileChange>,
        output_tx: mpsc::Sender<Vec<FileChange>>,
    ) -> Self {
        Self {
            aggregator: EventAggregator::new(batch_size),
            batch_size,
            batch_timeout,
            input_rx,
            output_tx,
        }
    }

    /// Run the batch processor
    pub async fn run(mut self) {
        let mut timeout_interval = interval(self.batch_timeout);

        loop {
            tokio::select! {
                Some(event) = self.input_rx.recv() => {
                    self.aggregator.add_event(event);

                    // Check if batch is full
                    if self.aggregator.len() >= self.batch_size {
                        self.flush_batch().await;
                    }
                }
                _ = timeout_interval.tick() => {
                    // Flush on timeout if there are events
                    if !self.aggregator.is_empty() {
                        self.flush_batch().await;
                    }
                }
                else => {
                    // Channel closed
                    break;
                }
            }
        }

        // Final flush
        if !self.aggregator.is_empty() {
            self.flush_batch().await;
        }
    }

    /// Flush the current batch
    async fn flush_batch(&mut self) {
        let batch = self.aggregator.aggregate_events();
        if !batch.is_empty() {
            debug!("Flushing batch of {} events", batch.len());
            let _ = self.output_tx.send(batch).await;
        }
    }
}

/// Coalesces rapid file modifications into single events
pub struct ChangeCoalescer {
    /// Time window for coalescing
    coalesce_window: Duration,
    /// Recent events by path
    recent_events: Arc<DashMap<PathBuf, CoalescedChange>>,
}

impl ChangeCoalescer {
    /// Create a new change coalescer
    pub fn new(coalesce_window: Duration) -> Self {
        Self {
            coalesce_window,
            recent_events: Arc::new(DashMap::new()),
        }
    }

    /// Process an event and potentially coalesce it
    pub fn process(&self, event: FileChange) -> Option<FileChange> {
        let path = event.path().clone();

        // Check for recent events on the same path
        if let Some(mut recent) = self.recent_events.get_mut(&path) {
            let time_since = SystemTime::now()
                .duration_since(recent.last_seen)
                .unwrap_or_default();

            if time_since < self.coalesce_window {
                // Coalesce events
                recent.coalesce(event);
                return None; // Event was coalesced
            }
        }

        // New event or outside coalesce window
        let coalesced = CoalescedChange::new(event.clone());
        self.recent_events.insert(path, coalesced);

        Some(event)
    }

    /// Clean up old entries
    pub async fn cleanup(&self) {
        let now = SystemTime::now();
        let window = self.coalesce_window * 2; // Keep entries for 2x window

        self.recent_events
            .retain(|_, change| now.duration_since(change.last_seen).unwrap_or_default() < window);
    }
}

/// Represents a coalesced change
#[derive(Debug, Clone)]
struct CoalescedChange {
    first_event: FileChange,
    last_seen: SystemTime,
    event_count: u32,
}

impl CoalescedChange {
    fn new(event: FileChange) -> Self {
        Self {
            first_event: event,
            last_seen: SystemTime::now(),
            event_count: 1,
        }
    }

    fn coalesce(&mut self, event: FileChange) {
        // Keep the most recent event type
        self.first_event = event;
        self.last_seen = SystemTime::now();
        self.event_count += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::FileMetadata;

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

    #[test]
    fn test_event_aggregator() {
        let mut aggregator = EventAggregator::new(5);

        // Add events with different priorities
        let path1 = PathBuf::from("file1.rs");
        let path2 = PathBuf::from("file2.rs");

        aggregator.add_event(FileChange::Created(
            path1.clone(),
            FileMetadata::new(100, SystemTime::now(), 0o644),
        ));
        aggregator.add_event(FileChange::Deleted(path2.clone()));
        aggregator.add_event(FileChange::Modified(
            path1.clone(),
            crate::events::DiffStats::new(vec![], vec![], vec![]),
        ));

        let events = aggregator.aggregate_events();

        // Delete should come first (highest priority)
        assert!(matches!(&events[0], FileChange::Deleted(p) if p == &path2));
        // Modified should come next, duplicate path1 should be filtered
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn test_prioritized_event_ordering() {
        let mut heap = BinaryHeap::new();

        let event1 = PrioritizedEvent {
            priority: 2,
            timestamp: SystemTime::now(),
            event: FileChange::Created(
                PathBuf::from("test.rs"),
                FileMetadata::new(100, SystemTime::now(), 0o644),
            ),
        };

        let event2 = PrioritizedEvent {
            priority: 0,
            timestamp: SystemTime::now(),
            event: FileChange::Deleted(PathBuf::from("test.rs")),
        };

        heap.push(event1.clone());
        heap.push(event2.clone());

        // Priority 0 (delete) should come first
        let first = heap.pop().expect("test setup failed");
        assert_eq!(first.priority, 0);
    }

    #[tokio::test]
    async fn test_batch_processor() {
        let (input_tx, input_rx) = mpsc::channel(10);
        let (output_tx, mut output_rx) = mpsc::channel(10);

        let processor = BatchProcessor::new(3, Duration::from_millis(100), input_rx, output_tx);

        // Run processor in background
        tokio::spawn(processor.run());

        // Send events
        for i in 0..3 {
            let path = PathBuf::from(format!("file{i}.rs"));
            let event = FileChange::Created(path, FileMetadata::new(100, SystemTime::now(), 0o644));
            input_tx.send(event).await.expect("test setup failed");
        }

        // Should receive batch after reaching batch size
        let batch = output_rx.recv().await.expect("test setup failed");
        assert_eq!(batch.len(), 3);
    }

    #[test]
    fn test_change_coalescer() {
        let coalescer = ChangeCoalescer::new(Duration::from_millis(50));
        let path = PathBuf::from("test.rs");

        let event1 = FileChange::Modified(
            path.clone(),
            crate::events::DiffStats::new(vec![], vec![], vec![]),
        );

        // First event should pass through
        assert!(coalescer.process(event1.clone()).is_some());

        // Immediate second event should be coalesced
        assert!(coalescer.process(event1.clone()).is_none());
    }
}
