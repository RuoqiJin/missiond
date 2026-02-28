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

/// Extract the last assistant text from a JSONL file.
/// Reads from end of file for efficiency (avoids loading entire file).
/// Returns None if no assistant text found or file is unreadable.
pub async fn extract_last_assistant_text(file_path: &Path) -> Option<String> {
    let file = fs::File::open(file_path).await.ok()?;
    let metadata = file.metadata().await.ok()?;
    let file_size = metadata.len();
    if file_size == 0 {
        return None;
    }

    // Read last 64KB chunk from end — sufficient for most assistant responses
    const TAIL_SIZE: u64 = 64 * 1024;
    let seek_pos = file_size.saturating_sub(TAIL_SIZE);
    let mut reader = BufReader::new(file);
    reader.seek(SeekFrom::Start(seek_pos)).await.ok()?;

    // If we seeked into the middle of a line, skip the partial first line
    if seek_pos > 0 {
        let mut partial = String::new();
        reader.read_line(&mut partial).await.ok()?;
    }

    let mut last_assistant_text: Option<String> = None;
    let mut lines = reader.lines();

    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        // Parse line — skip broken/half-written lines
        let msg: CCMessageLine = match serde_json::from_str(&line) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if msg.message.role != "assistant" {
            continue;
        }
        // Extract text from content (string or array of content blocks)
        if let Some(text) = extract_text_from_content(&msg.message.content) {
            if !text.is_empty() {
                last_assistant_text = Some(text);
            }
        }
    }

    last_assistant_text
}

/// Extract text from Claude message content value.
/// Content can be a plain string or an array of content blocks.
fn extract_text_from_content(content: &serde_json::Value) -> Option<String> {
    match content {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Array(blocks) => {
            let mut texts = Vec::new();
            for block in blocks {
                if let Some(block_type) = block.get("type").and_then(|t| t.as_str()) {
                    if block_type == "text" {
                        if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                            texts.push(text.to_string());
                        }
                    }
                }
            }
            if texts.is_empty() {
                None
            } else {
                Some(texts.join("\n"))
            }
        }
        _ => None,
    }
}

/// Check if a JSONL file has a completed turn (turn_duration) with no subsequent user message.
/// Used as a compensating signal when PTY state machine misses Idle events (e.g. after compaction).
/// Reads from end of file for efficiency.
pub async fn jsonl_has_completed_turn(file_path: &Path) -> bool {
    let file = match fs::File::open(file_path).await {
        Ok(f) => f,
        Err(_) => return false,
    };
    let metadata = match file.metadata().await {
        Ok(m) => m,
        Err(_) => return false,
    };
    let file_size = metadata.len();
    if file_size == 0 {
        return false;
    }

    // Read last 32KB — turn_duration + any trailing user message should fit
    const TAIL_SIZE: u64 = 32 * 1024;
    let seek_pos = file_size.saturating_sub(TAIL_SIZE);
    let mut reader = BufReader::new(file);
    if reader.seek(SeekFrom::Start(seek_pos)).await.is_err() {
        return false;
    }
    // Skip partial first line if we seeked into the middle
    if seek_pos > 0 {
        let mut partial = String::new();
        let _ = reader.read_line(&mut partial).await;
    }

    let mut saw_turn_duration = false;
    let mut saw_user_after = false;
    let mut lines = reader.lines();

    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        let val: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue, // skip half-written lines
        };
        let msg_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let subtype = val.get("subtype").and_then(|v| v.as_str()).unwrap_or("");

        if msg_type == "system" && subtype == "turn_duration" {
            saw_turn_duration = true;
            saw_user_after = false; // reset — only care about user messages AFTER this
        } else if saw_turn_duration && (msg_type == "human" || msg_type == "user") {
            saw_user_after = true; // new user message sent after turn completed — turn is not final
        }
    }

    // Turn completed and no new user message followed
    saw_turn_duration && !saw_user_after
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

/// Read new content from file since last position — returns raw JSON values.
/// Unlike read_new_lines, this captures ALL JSONL line types (progress, system, etc.)
pub async fn read_new_lines_raw(
    file_path: &Path,
    from_position: u64,
) -> anyhow::Result<(Vec<serde_json::Value>, u64)> {
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
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
            lines.push(val);
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
