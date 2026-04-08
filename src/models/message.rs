use crate::api::types::GraphChatMessage;
use crate::models::chat::strip_html;

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub id: String,
    pub sender_name: String,
    pub sender_id: Option<String>,
    pub content: String,
    pub timestamp: String,
    pub is_from_me: bool,
    pub is_system: bool,
}

impl ChatMessage {
    pub fn from_graph(msg: GraphChatMessage, my_user_id: &str) -> Self {
        let (sender_name, sender_id) = msg
            .from
            .as_ref()
            .and_then(|f| f.user.as_ref())
            .map(|u| {
                (
                    u.display_name
                        .clone()
                        .unwrap_or_else(|| "Unknown".into()),
                    Some(u.id.clone()),
                )
            })
            .or_else(|| {
                msg.from.as_ref().and_then(|f| f.application.as_ref()).map(
                    |a| {
                        (
                            a.display_name
                                .clone()
                                .unwrap_or_else(|| "Bot".into()),
                            Some(a.id.clone()),
                        )
                    },
                )
            })
            .unwrap_or(("System".into(), None));

        let is_from_me = sender_id.as_deref() == Some(my_user_id);
        let is_system = msg.message_type.as_deref() != Some("message");

        let content =
            if msg.body.content_type.as_deref() == Some("html") {
                strip_html(&msg.body.content)
            } else {
                msg.body.content.clone()
            };

        ChatMessage {
            id: msg.id,
            sender_name,
            sender_id,
            content,
            timestamp: msg.created_date_time.unwrap_or_default(),
            is_from_me,
            is_system,
        }
    }
}
