// Integration tests for infrastructure module
// These tests verify file locking behavior for concurrent infrastructure initialization

use fs2::FileExt;
use std::fs::File;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

/// RAII guard for file locking (duplicated from infrastructure.rs for testing)
#[derive(Debug)]
struct LockGuard {
    file: File,
}

impl LockGuard {
    fn try_lock_exclusive(file: File, timeout: Duration) -> anyhow::Result<Self> {
        let start = std::time::Instant::now();

        loop {
            match file.try_lock_exclusive() {
                Ok(()) => return Ok(Self { file }),
                Err(e) if start.elapsed() >= timeout => {
                    return Err(anyhow::anyhow!("Timeout waiting for lock: {e}"));
                }
                Err(_) => {
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        // Best effort unlock
        let _ = FileExt::unlock(&self.file);
    }
}

#[test]
fn test_lock_file_prevents_concurrent_access() {
    let temp_dir = TempDir::new().unwrap();
    let lock_path = temp_dir.path().join(".infrastructure.lock");

    // Create lock file
    let lock_file1 = File::options()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .unwrap();

    // Acquire first lock
    let guard1 = LockGuard::try_lock_exclusive(lock_file1, Duration::from_secs(1)).unwrap();

    // Try to acquire second lock - should timeout
    let lock_file2 = File::options()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .unwrap();

    let result = LockGuard::try_lock_exclusive(lock_file2, Duration::from_millis(100));
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Timeout"));

    // Drop first lock
    drop(guard1);

    // Now second lock should succeed
    let lock_file3 = File::options()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .unwrap();

    let guard3 = LockGuard::try_lock_exclusive(lock_file3, Duration::from_secs(1));
    assert!(guard3.is_ok());
}

#[test]
fn test_lock_guard_auto_releases() {
    let temp_dir = TempDir::new().unwrap();
    let lock_path = temp_dir.path().join(".infrastructure.lock");

    {
        let lock_file = File::options()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .unwrap();

        let _guard = LockGuard::try_lock_exclusive(lock_file, Duration::from_secs(1)).unwrap();
        // Guard drops here
    }

    // Lock should be released, new lock should succeed immediately
    let lock_file2 = File::options()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .unwrap();

    let guard2 = LockGuard::try_lock_exclusive(lock_file2, Duration::from_millis(100));
    assert!(guard2.is_ok());
}

#[test]
fn test_concurrent_lock_attempts_from_multiple_threads() {
    let temp_dir = TempDir::new().unwrap();
    let lock_path = Arc::new(temp_dir.path().join(".infrastructure.lock"));

    // Create lock file
    std::fs::write(&*lock_path, b"").unwrap();

    let mut handles = vec![];
    let success_count = Arc::new(Mutex::new(0));

    // Spawn 5 threads trying to acquire lock
    for _ in 0..5 {
        let path = Arc::clone(&lock_path);
        let count = Arc::clone(&success_count);

        let handle = thread::spawn(move || {
            let lock_file = File::options()
                .create(true)
                .write(true)
                .truncate(false)
                .open(&*path)
                .unwrap();

            if let Ok(_guard) = LockGuard::try_lock_exclusive(lock_file, Duration::from_millis(500))
            {
                // Hold lock briefly
                thread::sleep(Duration::from_millis(50));
                let mut count = count.lock().unwrap();
                *count += 1;
            }
        });

        handles.push(handle);
        // Stagger thread starts slightly
        thread::sleep(Duration::from_millis(10));
    }

    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }

    // At least one thread should have succeeded
    let final_count = *success_count.lock().unwrap();
    assert!(
        final_count >= 1,
        "At least one thread should acquire the lock"
    );
}
