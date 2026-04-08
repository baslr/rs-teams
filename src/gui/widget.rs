use iced::widget::text;
use iced::Element;

/// Format an ISO 8601 timestamp to a short HH:MM display
pub fn format_timestamp(ts: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(ts)
        .map(|dt| dt.format("%H:%M").to_string())
        .unwrap_or_else(|_| ts.chars().take(16).collect())
}

/// Initials avatar placeholder: takes a name, returns first letter(s)
pub fn initials(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|w| w.chars().next())
        .take(2)
        .collect::<String>()
        .to_uppercase()
}

/// A small text badge showing initials
pub fn avatar_text<'a, M: 'a>(name: &str) -> Element<'a, M> {
    text(initials(name)).size(12).into()
}
