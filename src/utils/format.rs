use std::time::{SystemTime, UNIX_EPOCH};

pub fn format_relative_time(datetime_str: &str) -> String {
    if datetime_str.is_empty() {
        return String::new();
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let Some(dt) = chrono::NaiveDateTime::parse_from_str(datetime_str, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(datetime_str, "%Y-%m-%dT%H:%M:%S"))
        .ok()
    else {
        return String::new();
    };
    let ts = dt.and_utc().timestamp();
    let diff = now - ts;
    if diff < 0 {
        return String::new();
    }
    let minutes = diff / 60;
    let hours = minutes / 60;
    let days = hours / 24;

    if minutes < 1 {
        return "just now".to_string();
    } else if minutes < 60 {
        return format!("{} minute{} ago", minutes, if minutes == 1 { "" } else { "s" });
    } else if hours < 24 {
        return format!("{} hour{} ago", hours, if hours == 1 { "" } else { "s" });
    } else if days < 7 {
        return format!("{} day{} ago", days, if days == 1 { "" } else { "s" });
    } else {
        return dt.format("%b %d").to_string();
    }
}

pub fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut truncated = s[..s.floor_char_boundary(max)].to_string();
        truncated.push_str("...");
        truncated
    }
}
