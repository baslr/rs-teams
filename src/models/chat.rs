use crate::api::types::GraphChat;

#[derive(Debug, Clone)]
pub enum ChatType {
    OneOnOne,
    Group,
    Meeting,
}

#[derive(Debug, Clone)]
pub struct Chat {
    pub id: String,
    pub display_name: String,
    pub chat_type: ChatType,
    pub last_message_preview: Option<String>,
    pub last_activity: Option<String>,
    pub members: Vec<ChatMember>,
}

#[derive(Debug, Clone)]
pub struct ChatMember {
    pub user_id: String,
    pub display_name: String,
}

impl Chat {
    pub fn from_graph(graph: GraphChat, my_user_id: &str) -> Self {
        let chat_type = match graph.chat_type.as_str() {
            "oneOnOne" => ChatType::OneOnOne,
            "group" => ChatType::Group,
            _ => ChatType::Meeting,
        };

        let display_name = graph.topic.clone().unwrap_or_else(|| {
            graph
                .members
                .as_ref()
                .and_then(|members| {
                    members
                        .iter()
                        .find(|m| m.user_id.as_deref() != Some(my_user_id))
                        .and_then(|m| m.display_name.clone())
                })
                .unwrap_or_else(|| "Chat".into())
        });

        let last_message_preview = graph
            .last_message_preview
            .as_ref()
            .map(|p| strip_html(&p.body.content));

        let members = graph
            .members
            .unwrap_or_default()
            .into_iter()
            .map(|m| ChatMember {
                user_id: m.user_id.unwrap_or_default(),
                display_name: m.display_name.unwrap_or_else(|| "Unknown".into()),
            })
            .collect();

        Chat {
            id: graph.id,
            display_name,
            chat_type,
            last_message_preview,
            last_activity: graph.last_updated_date_time,
            members,
        }
    }
}

/// Naive HTML tag stripper
pub fn strip_html(s: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(c),
            _ => {}
        }
    }
    result.trim().to_string()
}
