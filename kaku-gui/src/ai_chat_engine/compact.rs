//! Tool-output compaction for the AI chat agent loop.

use crate::ai_client::ApiMessage;
use std::path::PathBuf;

const FS_READ_CAP: usize = 200;
const GREP_CAP: usize = 50;
const GREP_HEAD: usize = 30;
const GREP_TAIL: usize = 10;
const BASH_CAP: usize = 100;
const BASH_HEAD: usize = 50;
const BASH_TAIL: usize = 30;

fn compact_tool_content(tool_name: &str, content: &str) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();
    match tool_name {
        "fs_read" if lines.len() > FS_READ_CAP => Some(format!(
            "[fs_read: {} lines total, showing first {}]\n{}",
            lines.len(),
            FS_READ_CAP,
            lines[..FS_READ_CAP].join("\n")
        )),
        "grep_search" | "fs_search" | "fs_list" if lines.len() > GREP_CAP => {
            let total = lines.len();
            Some(format!(
                "{}\n[{} lines elided]\n{}",
                lines[..GREP_HEAD].join("\n"),
                total - GREP_HEAD - GREP_TAIL,
                lines[total - GREP_TAIL..].join("\n")
            ))
        }
        "shell_exec" | "shell_bg" if lines.len() > BASH_CAP => {
            let total = lines.len();
            Some(format!(
                "{}\n[{} lines elided]\n{}",
                lines[..BASH_HEAD].join("\n"),
                total - BASH_HEAD - BASH_TAIL,
                lines[total - BASH_TAIL..].join("\n")
            ))
        }
        _ => None,
    }
}

/// Apply micro-compaction to all tool-result messages in `messages`.
pub(crate) fn micro_compact(
    messages: &mut Vec<ApiMessage>,
    round: usize,
    outputs_dir: Option<&PathBuf>,
) {
    for (idx, msg) in messages.iter_mut().enumerate() {
        let role = msg.0.get("role").and_then(|v| v.as_str()).unwrap_or("");
        if role != "tool" {
            continue;
        }
        let content = match msg.0.get("content").and_then(|v| v.as_str()) {
            Some(c) => c.to_string(),
            None => continue,
        };
        if content.starts_with("Error:") || content.starts_with('[') {
            continue;
        }
        let tool_name = msg.0.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let Some(compacted) = compact_tool_content(tool_name, &content) else {
            continue;
        };

        if let Some(dir) = outputs_dir {
            if std::fs::create_dir_all(dir).is_ok() {
                let fname = format!("r{}-{}.txt", round, idx);
                let _ = std::fs::write(dir.join(fname), content.as_bytes());
            }
        }

        if let Some(obj) = msg.0.as_object_mut() {
            obj.insert("content".to_string(), serde_json::Value::String(compacted));
        }
    }
}
