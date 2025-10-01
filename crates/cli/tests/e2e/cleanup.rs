//! Cleanup verification utilities for E2E tests

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Command;

/// Track resources created during tests to verify cleanup
#[derive(Debug, Default)]
pub struct CleanupTracker {
    temp_dirs: Vec<PathBuf>,
    container_names: Vec<String>,
}

impl CleanupTracker {
    /// Create a new cleanup tracker
    pub fn new() -> Self {
        Self::default()
    }

    /// Track a temporary directory that should be cleaned up
    pub fn track_temp_dir(&mut self, path: PathBuf) {
        self.temp_dirs.push(path);
    }

    /// Track a container name that should be cleaned up
    pub fn track_container(&mut self, name: String) {
        self.container_names.push(name);
    }

    /// Verify all tracked resources have been cleaned up
    pub fn verify_cleanup(&self) -> Result<()> {
        let mut errors = Vec::new();

        // Check temp directories
        for dir in &self.temp_dirs {
            if dir.exists() {
                errors.push(format!("Temp directory still exists: {}", dir.display()));
            }
        }

        // Check containers
        for name in &self.container_names {
            if container_exists(name)? {
                errors.push(format!("Container still exists: {name}"));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "Cleanup verification failed:\n  - {}",
                errors.join("\n  - ")
            ))
        }
    }
}

/// Check if a Docker container exists
fn container_exists(name: &str) -> Result<bool> {
    let output = Command::new("docker")
        .args([
            "ps",
            "-a",
            "--filter",
            &format!("name={name}"),
            "--format",
            "{{.Names}}",
        ])
        .output()
        .context("Failed to check for container")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.lines().any(|line| line.trim() == name))
}

/// Verify no orphaned test containers remain
pub fn verify_no_orphaned_containers() -> Result<()> {
    let output = Command::new("docker")
        .args([
            "ps",
            "-a",
            "--filter",
            "name=qdrant-test-",
            "--format",
            "{{.Names}}",
        ])
        .output()
        .context("Failed to list test containers")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let orphaned: Vec<&str> = stdout.lines().filter(|line| !line.is_empty()).collect();

    if orphaned.is_empty() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "Found {} orphaned test container(s):\n  - {}",
            orphaned.len(),
            orphaned.join("\n  - ")
        ))
    }
}

/// Verify no orphaned temp directories remain
pub fn verify_no_orphaned_temp_dirs() -> Result<()> {
    let patterns = vec!["qdrant-test-*", "codesearch-test-*"];
    let mut orphaned = Vec::new();

    for pattern in patterns {
        let glob_pattern = format!("/tmp/{pattern}");
        if let Ok(entries) = glob::glob(&glob_pattern) {
            for entry in entries.flatten() {
                orphaned.push(entry.display().to_string());
            }
        }
    }

    if orphaned.is_empty() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "Found {} orphaned temp director(ies):\n  - {}",
            orphaned.len(),
            orphaned.join("\n  - ")
        ))
    }
}

/// Clean up all orphaned test resources (for manual cleanup)
pub fn cleanup_all_orphaned_resources() -> Result<()> {
    // Clean up containers
    let output = Command::new("docker")
        .args([
            "ps",
            "-a",
            "--filter",
            "name=qdrant-test-",
            "--format",
            "{{.Names}}",
        ])
        .output()
        .context("Failed to list test containers")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    for container in stdout.lines().filter(|line| !line.is_empty()) {
        let _ = Command::new("docker")
            .args(["rm", "-f", container])
            .output();
    }

    // Clean up temp directories
    let patterns = vec!["qdrant-test-*", "codesearch-test-*"];
    for pattern in patterns {
        let glob_pattern = format!("/tmp/{pattern}");
        if let Ok(entries) = glob::glob(&glob_pattern) {
            for entry in entries.flatten() {
                let _ = std::fs::remove_dir_all(entry);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cleanup_tracker_detects_existing_dirs() {
        let temp_dir = std::env::temp_dir().join(format!("test-cleanup-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let mut tracker = CleanupTracker::new();
        tracker.track_temp_dir(temp_dir.clone());

        // Should fail because directory exists
        assert!(tracker.verify_cleanup().is_err());

        // Clean up and verify again
        std::fs::remove_dir_all(&temp_dir).unwrap();
        assert!(tracker.verify_cleanup().is_ok());
    }

    #[test]
    fn test_cleanup_tracker_passes_for_removed_dirs() {
        let temp_dir = std::env::temp_dir().join(format!("test-cleanup-{}", uuid::Uuid::new_v4()));

        let mut tracker = CleanupTracker::new();
        tracker.track_temp_dir(temp_dir);

        // Should pass because directory never existed
        assert!(tracker.verify_cleanup().is_ok());
    }
}
