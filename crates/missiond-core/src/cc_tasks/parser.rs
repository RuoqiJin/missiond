//! Claude Code JSONL Parser
//!
//! Parses sessions-index.json and .jsonl conversation files

use super::types::*;
use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;
use std::path::Path;
use tokio::fs;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, BufReader, SeekFrom};

/// Parse sessions-index.json
pub async fn parse_sessions_index(file_path: &Path) -> Option<CCSessionIndex> {
    let content = fs::read_to_string(file_path).await.ok()?;
    serde_json::from_str(&content).ok()
}

/// Parse the last N lines of a JSONL file to extract todos
/// Returns the most recent todos array found
pub async fn parse_last_todos(file_path: &Path, max_lines: usize) -> Vec<CCTask> {
    let content = match fs::read_to_string(file_path).await {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
    let start_idx = lines.len().saturating_sub(max_lines);

    // Parse lines from end to find most recent todos
    for line in lines[start_idx..].iter().rev() {
        if let Ok(msg) = serde_json::from_str::<CCMessageLine>(line) {
            if let Some(todos) = msg.todos {
                if !todos.is_empty() {
                    return todos;
                }
            }
        }
    }

    Vec::new()
}

/// Parse JSONL file using streaming (memory efficient)
pub async fn parse_jsonl_stream<F>(file_path: &Path, mut on_line: F) -> anyhow::Result<()>
where
    F: FnMut(CCMessageLine),
{
    let file = fs::File::open(file_path).await?;
    let reader = BufReader::new(file);
    let mut lines = reader.lines();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(msg) = serde_json::from_str::<CCMessageLine>(&line) {
            on_line(msg);
        }
    }

    Ok(())
}

/// Convert index entry to CCSession with tasks
pub async fn entry_to_session(entry: &CCSessionIndexEntry) -> CCSession {
    let tasks = parse_last_todos(Path::new(&entry.full_path), 100).await;
    let now = Utc::now();
    let modified = DateTime::parse_from_rfc3339(&entry.modified)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or(now);
    let created = DateTime::parse_from_rfc3339(&entry.created)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or(now);
    let five_minutes_ago = now - Duration::minutes(5);

    // Extract project name from path (last 2 segments)
    let path_parts: Vec<&str> = entry.project_path.split('/').filter(|s| !s.is_empty()).collect();
    let project_name = if path_parts.len() >= 2 {
        format!("{}/{}", path_parts[path_parts.len() - 2], path_parts[path_parts.len() - 1])
    } else if !path_parts.is_empty() {
        path_parts.last().unwrap().to_string()
    } else {
        entry.project_path.clone()
    };

    CCSession {
        session_id: entry.session_id.clone(),
        project_path: entry.project_path.clone(),
        project_name,
        summary: entry.summary.clone(),
        modified,
        created,
        git_branch: entry.git_branch.clone(),
        tasks,
        message_count: entry.message_count,
        is_active: modified > five_minutes_ago,
        full_path: entry.full_path.clone(),
    }
}

/// Get file size in bytes
pub async fn get_file_size(file_path: &Path) -> u64 {
    fs::metadata(file_path)
        .await
        .map(|m| m.len())
        .unwrap_or(0)
}

/// Read new content from file since last position
pub async fn read_new_lines(
    file_path: &Path,
    from_position: u64,
) -> anyhow::Result<(Vec<CCMessageLine>, u64)> {
    let metadata = fs::metadata(file_path).await?;
    let file_size = metadata.len();

    if file_size <= from_position {
        return Ok((Vec::new(), from_position));
    }

    let mut file = fs::File::open(file_path).await?;
    file.seek(SeekFrom::Start(from_position)).await?;

    let mut buffer = vec![0u8; (file_size - from_position) as usize];
    file.read_exact(&mut buffer).await?;

    let content = String::from_utf8_lossy(&buffer);
    let mut lines = Vec::new();

    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(msg) = serde_json::from_str::<CCMessageLine>(line) {
            lines.push(msg);
        }
    }

    Ok((lines, file_size))
}

/// Diff two task arrays to find changes
pub fn diff_tasks(previous: &[CCTask], current: &[CCTask]) -> TaskDiff {
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut status_changed = Vec::new();

    // Map by content for comparison
    let prev_map: HashMap<&str, &CCTask> = previous.iter().map(|t| (t.content.as_str(), t)).collect();
    let curr_map: HashMap<&str, &CCTask> = current.iter().map(|t| (t.content.as_str(), t)).collect();

    // Find added and status changes
    for task in current {
        if let Some(prev) = prev_map.get(task.content.as_str()) {
            if prev.status != task.status {
                status_changed.push(TaskStatusChange {
                    task: task.clone(),
                    previous_status: prev.status,
                });
            }
        } else {
            added.push(task.clone());
        }
    }

    // Find removed
    for task in previous {
        if !curr_map.contains_key(task.content.as_str()) {
            removed.push(task.clone());
        }
    }

    TaskDiff {
        added,
        removed,
        status_changed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_tasks_added() {
        let prev = vec![];
        let curr = vec![CCTask {
            content: "Task 1".to_string(),
            status: CCTaskStatus::Pending,
            active_form: None,
        }];

        let diff = diff_tasks(&prev, &curr);
        assert_eq!(diff.added.len(), 1);
        assert_eq!(diff.removed.len(), 0);
        assert_eq!(diff.status_changed.len(), 0);
    }

    #[test]
    fn test_diff_tasks_removed() {
        let prev = vec![CCTask {
            content: "Task 1".to_string(),
            status: CCTaskStatus::Pending,
            active_form: None,
        }];
        let curr = vec![];

        let diff = diff_tasks(&prev, &curr);
        assert_eq!(diff.added.len(), 0);
        assert_eq!(diff.removed.len(), 1);
        assert_eq!(diff.status_changed.len(), 0);
    }

    #[test]
    fn test_diff_tasks_status_changed() {
        let prev = vec![CCTask {
            content: "Task 1".to_string(),
            status: CCTaskStatus::Pending,
            active_form: None,
        }];
        let curr = vec![CCTask {
            content: "Task 1".to_string(),
            status: CCTaskStatus::InProgress,
            active_form: None,
        }];

        let diff = diff_tasks(&prev, &curr);
        assert_eq!(diff.added.len(), 0);
        assert_eq!(diff.removed.len(), 0);
        assert_eq!(diff.status_changed.len(), 1);
        assert_eq!(diff.status_changed[0].previous_status, CCTaskStatus::Pending);
    }
}
