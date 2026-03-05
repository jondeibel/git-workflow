use std::collections::HashMap;
use std::process::Command;

/// PR information from gh CLI.
pub struct PrInfo {
    pub number: u64,
    pub state: String,
}

/// Batch-fetch PR status for all branches via a single gh CLI call.
/// Returns a map of branch_name -> PrInfo.
/// Returns empty map if gh is not available or fails.
pub fn batch_pr_status() -> HashMap<String, PrInfo> {
    let mut result = HashMap::new();

    let output = Command::new("gh")
        .args([
            "pr", "list", "--state", "all", "--json",
            "headRefName,number,state", "--limit", "100",
        ])
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return result,
    };

    let stdout = String::from_utf8_lossy(&output.stdout);

    for chunk in stdout.split('{') {
        if let (Some(branch), Some(number), Some(state)) = (
            extract_json_string(chunk, "headRefName"),
            extract_json_number(chunk, "number"),
            extract_json_string(chunk, "state"),
        ) {
            result.insert(branch, PrInfo { number, state });
        }
    }

    result
}

/// Check if a specific branch has a merged PR via the batched results.
pub fn is_branch_merged(pr_map: &HashMap<String, PrInfo>, branch: &str) -> bool {
    pr_map
        .get(branch)
        .is_some_and(|info| info.state == "MERGED")
}

/// Format PR status for display.
pub fn format_pr_status(info: &PrInfo) -> String {
    match info.state.as_str() {
        "OPEN" => format!("PR #{} open", info.number),
        "CLOSED" => format!("PR #{} closed", info.number),
        "MERGED" => format!("PR #{} merged", info.number),
        _ => format!("PR #{}", info.number),
    }
}

fn extract_json_string(text: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{key}\":\"");
    let start = text.find(&pattern)? + pattern.len();
    let end = text[start..].find('"')? + start;
    Some(text[start..end].to_string())
}

fn extract_json_number(text: &str, key: &str) -> Option<u64> {
    let pattern = format!("\"{key}\":");
    let start = text.find(&pattern)? + pattern.len();
    let num_str: String = text[start..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    num_str.parse().ok()
}
