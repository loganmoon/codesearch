//! Integration tests for the file watcher
//!
//! These tests use temporary directories and real filesystem operations
//! to validate the watcher's behavior in realistic scenarios.

use codesearch_watcher::{FileChange, FileWatcher, GitRepository, WatcherConfig};
use git2::Repository;
use std::path::PathBuf;
use std::time::Duration;
use tempfile::TempDir;
use tokio::time::timeout;

/// Helper to create a test directory with Git repository
async fn setup_git_repo() -> (TempDir, Repository) {
    let temp_dir = TempDir::new().unwrap();
    let repo = Repository::init(temp_dir.path()).unwrap();

    // Configure test repo
    let mut config = repo.config().unwrap();
    config.set_str("user.name", "Test User").unwrap();
    config.set_str("user.email", "test@example.com").unwrap();

    // Create initial commit
    let sig = repo.signature().unwrap();
    let tree_id = {
        let mut index = repo.index().unwrap();
        index.write_tree().unwrap()
    };
    let tree = repo.find_tree(tree_id).unwrap();
    let _oid = repo
        .commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
        .unwrap();

    drop(tree);
    drop(sig);

    (temp_dir, repo)
}

/// Helper to create a test file
async fn create_test_file(dir: &TempDir, name: &str, content: &str) -> PathBuf {
    let path = dir.path().join(name);
    tokio::fs::write(&path, content).await.unwrap();
    path
}

#[tokio::test]
async fn test_file_creation_detection() {
    let temp_dir = TempDir::new().unwrap();

    let config = WatcherConfig::builder().debounce_ms(50).build();

    let mut watcher = FileWatcher::new(config).unwrap();
    let mut events = watcher.watch(temp_dir.path()).await.unwrap();

    // Wait a bit for watcher to stabilize
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Create a file
    let test_file = create_test_file(&temp_dir, "test.rs", "fn main() {}").await;

    // Wait for event with timeout
    let event = timeout(Duration::from_secs(2), events.recv())
        .await
        .unwrap()
        .unwrap();

    match event {
        FileChange::Created(path, _) => {
            assert_eq!(path, test_file);
        }
        _ => panic!("Expected Created event, got {event:?}"),
    }
}

#[tokio::test]
async fn test_file_modification_detection() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = create_test_file(&temp_dir, "test.rs", "fn main() {}").await;

    let config = WatcherConfig::builder().debounce_ms(50).build();

    let mut watcher = FileWatcher::new(config).unwrap();
    let mut events = watcher.watch(temp_dir.path()).await.unwrap();

    // Wait a bit for watcher to stabilize
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Modify the file
    tokio::fs::write(&test_file, "fn main() { println!(\"Hello\"); }")
        .await
        .unwrap();

    // Wait for modification event
    let event = timeout(Duration::from_secs(2), events.recv())
        .await
        .unwrap()
        .unwrap();

    match event {
        FileChange::Modified(path, _) => {
            assert_eq!(path, test_file);
        }
        _ => panic!("Expected Modified event, got {event:?}"),
    }
}

#[tokio::test]
async fn test_file_deletion_detection() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = create_test_file(&temp_dir, "test.rs", "fn main() {}").await;

    let config = WatcherConfig::builder().debounce_ms(50).build();

    let mut watcher = FileWatcher::new(config).unwrap();
    let mut events = watcher.watch(temp_dir.path()).await.unwrap();

    // Wait a bit for watcher to stabilize
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Delete the file
    tokio::fs::remove_file(&test_file).await.unwrap();

    // Wait for deletion event
    let event = timeout(Duration::from_secs(2), events.recv())
        .await
        .unwrap()
        .unwrap();

    match event {
        FileChange::Deleted(path) => {
            assert_eq!(path, test_file);
        }
        _ => panic!("Expected Deleted event, got {event:?}"),
    }
}

#[tokio::test]
async fn test_debouncing() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = create_test_file(&temp_dir, "test.rs", "fn main() {}").await;

    let config = WatcherConfig::builder()
        .debounce_ms(200) // Longer debounce window
        .build();

    let mut watcher = FileWatcher::new(config).unwrap();
    let mut events = watcher.watch(temp_dir.path()).await.unwrap();

    // Wait for watcher to stabilize
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Rapid modifications
    for i in 0..5 {
        tokio::fs::write(&test_file, format!("fn main() {{ /* {i} */ }}"))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(30)).await;
    }

    // Should receive only one event due to debouncing
    let event = timeout(Duration::from_secs(2), events.recv())
        .await
        .unwrap()
        .unwrap();

    assert!(matches!(event, FileChange::Modified(_, _)));

    // Should not receive another event immediately
    let result = timeout(Duration::from_millis(500), events.recv()).await;
    assert!(result.is_err(), "Received unexpected additional event");
}

#[tokio::test]
async fn test_ignore_patterns() {
    let temp_dir = TempDir::new().unwrap();

    let config = WatcherConfig::builder()
        .debounce_ms(50)
        .add_ignore_pattern("*.log".to_string())
        .add_ignore_pattern("*.tmp".to_string())
        .build();

    let mut watcher = FileWatcher::new(config).unwrap();
    let mut events = watcher.watch(temp_dir.path()).await.unwrap();

    // Wait a bit for watcher to stabilize
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Create files
    create_test_file(&temp_dir, "test.rs", "fn main() {}").await;
    create_test_file(&temp_dir, "debug.log", "log content").await;
    create_test_file(&temp_dir, "temp.tmp", "temp content").await;

    // Should only receive event for test.rs
    let event = timeout(Duration::from_secs(2), events.recv())
        .await
        .unwrap()
        .unwrap();

    match event {
        FileChange::Created(path, _) => {
            assert!(path.ends_with("test.rs"));
        }
        _ => panic!("Expected Created event for test.rs"),
    }

    // Should not receive events for ignored files
    let result = timeout(Duration::from_millis(500), events.recv()).await;
    assert!(result.is_err(), "Received event for ignored file");
}

#[tokio::test]
async fn test_git_branch_detection() {
    let (temp_dir, repo) = setup_git_repo().await;

    let config = WatcherConfig::builder().debounce_ms(50).build();

    let mut watcher = FileWatcher::new(config).unwrap();
    watcher.init_git(temp_dir.path()).await.unwrap();

    // Check current branch
    let git_repo = GitRepository::open(temp_dir.path()).unwrap();
    let branch = git_repo.current_branch().unwrap();
    assert!(branch == "main" || branch == "master");

    // Create and switch to new branch
    let _sig = repo.signature().unwrap();
    let head = repo.head().unwrap();
    let oid = head.target().unwrap();
    let commit = repo.find_commit(oid).unwrap();
    repo.branch("test-branch", &commit, false).unwrap();

    git_repo.checkout_branch("test-branch").unwrap();

    let new_branch = git_repo.current_branch().unwrap();
    assert_eq!(new_branch, "test-branch");
}

#[tokio::test]
async fn test_recursive_watching() {
    let temp_dir = TempDir::new().unwrap();

    // Create nested directory structure
    let sub_dir = temp_dir.path().join("src").join("modules");
    tokio::fs::create_dir_all(&sub_dir).await.unwrap();

    let config = WatcherConfig::builder()
        .debounce_ms(50)
        .recursive_depth(10)
        .build();

    let mut watcher = FileWatcher::new(config).unwrap();
    let mut events = watcher.watch(temp_dir.path()).await.unwrap();

    // Wait a bit for watcher to stabilize
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Create file in nested directory
    let nested_file = sub_dir.join("module.rs");
    tokio::fs::write(&nested_file, "pub fn test() {}")
        .await
        .unwrap();

    // Should detect file in nested directory
    let event = timeout(Duration::from_secs(2), events.recv())
        .await
        .unwrap()
        .unwrap();

    match event {
        FileChange::Created(path, _) => {
            assert_eq!(path, nested_file);
        }
        _ => panic!("Expected Created event for nested file"),
    }
}

#[tokio::test]
async fn test_max_file_size_limit() {
    let temp_dir = TempDir::new().unwrap();

    let config = WatcherConfig::builder()
        .debounce_ms(50)
        .max_file_size(100) // 100 bytes limit
        .build();

    let mut watcher = FileWatcher::new(config).unwrap();
    let mut events = watcher.watch(temp_dir.path()).await.unwrap();

    // Wait a bit for watcher to stabilize
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Create small file (under limit)
    create_test_file(&temp_dir, "small.txt", "small").await;

    // Create large file (over limit)
    let large_content = "x".repeat(200);
    create_test_file(&temp_dir, "large.txt", &large_content).await;

    // Should only receive event for small file
    let event = timeout(Duration::from_secs(2), events.recv())
        .await
        .unwrap()
        .unwrap();

    match event {
        FileChange::Created(path, _) => {
            assert!(path.ends_with("small.txt"));
        }
        _ => panic!("Expected Created event for small.txt"),
    }

    // Should not receive event for large file
    let result = timeout(Duration::from_millis(500), events.recv()).await;
    assert!(
        result.is_err(),
        "Received event for file exceeding size limit"
    );
}

#[tokio::test]
async fn test_concurrent_modifications() {
    let temp_dir = TempDir::new().unwrap();

    let config = WatcherConfig::builder()
        .debounce_ms(100)
        .events_per_batch(5)
        .build();

    let mut watcher = FileWatcher::new(config).unwrap();
    let mut events = watcher.watch(temp_dir.path()).await.unwrap();

    // Wait a bit for watcher to stabilize
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Create multiple files concurrently
    let mut handles = vec![];
    for i in 0..5 {
        let dir = temp_dir.path().to_path_buf();
        let handle = tokio::spawn(async move {
            let path = dir.join(format!("file{i}.txt"));
            tokio::fs::write(&path, format!("content {i}"))
                .await
                .unwrap();
        });
        handles.push(handle);
    }

    // Wait for all files to be created
    for handle in handles {
        handle.await.unwrap();
    }

    // Should receive events for all files
    let mut received_files = std::collections::HashSet::new();
    for _ in 0..5 {
        if let Ok(Some(FileChange::Created(path, _))) =
            timeout(Duration::from_secs(3), events.recv()).await
        {
            received_files.insert(path.file_name().unwrap().to_str().unwrap().to_string());
        }
    }

    assert_eq!(
        received_files.len(),
        5,
        "Should receive events for all 5 files"
    );
}

#[tokio::test]
async fn test_gitignore_integration() {
    let (temp_dir, _repo) = setup_git_repo().await;

    // Create .gitignore file
    let gitignore_content = "*.log\ntarget/\n*.tmp";
    tokio::fs::write(temp_dir.path().join(".gitignore"), gitignore_content)
        .await
        .unwrap();

    let config = WatcherConfig::builder().debounce_ms(50).build();

    let mut watcher = FileWatcher::new(config).unwrap();
    let mut events = watcher.watch(temp_dir.path()).await.unwrap();

    // Wait a bit for watcher to stabilize
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Create files (some ignored by gitignore)
    create_test_file(&temp_dir, "main.rs", "fn main() {}").await;
    create_test_file(&temp_dir, "debug.log", "log content").await;

    // Create target directory with file
    let target_dir = temp_dir.path().join("target");
    tokio::fs::create_dir(&target_dir).await.unwrap();
    tokio::fs::write(target_dir.join("output"), "build output")
        .await
        .unwrap();

    // Should only receive event for main.rs
    let event = timeout(Duration::from_secs(2), events.recv())
        .await
        .unwrap()
        .unwrap();

    match event {
        FileChange::Created(path, _) => {
            assert!(path.ends_with("main.rs"));
        }
        _ => panic!("Expected Created event for main.rs"),
    }

    // Should not receive events for gitignored files
    let result = timeout(Duration::from_millis(500), events.recv()).await;
    assert!(result.is_err(), "Received event for gitignored file");
}
