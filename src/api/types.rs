use serde::{Deserialize, Serialize};

/// OData collection wrapper for paginated responses
#[derive(Debug, Deserialize)]
pub struct ODataCollection<T> {
    pub value: Vec<T>,
    #[serde(rename = "@odata.nextLink")]
    pub next_link: Option<String>,
    #[serde(rename = "@odata.count")]
    pub count: Option<i64>,
}

/// GET /me/chats response item
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphChat {
    pub id: String,
    pub topic: Option<String>,
    pub chat_type: String,
    pub created_date_time: Option<String>,
    pub last_updated_date_time: Option<String>,
    pub web_url: Option<String>,
    #[serde(default)]
    pub members: Option<Vec<GraphChatMember>>,
    pub last_message_preview: Option<GraphMessagePreview>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphChatMember {
    pub display_name: Option<String>,
    pub user_id: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphMessagePreview {
    pub id: String,
    pub created_date_time: Option<String>,
    pub body: GraphMessageBody,
    pub from: Option<GraphMessageFrom>,
}

/// GET /chats/{id}/messages response item
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphChatMessage {
    pub id: String,
    pub message_type: Option<String>,
    pub created_date_time: Option<String>,
    pub last_modified_date_time: Option<String>,
    pub body: GraphMessageBody,
    pub from: Option<GraphMessageFrom>,
    pub importance: Option<String>,
    #[serde(default)]
    pub attachments: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphMessageBody {
    pub content_type: Option<String>,
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct GraphMessageFrom {
    pub user: Option<GraphUserIdentity>,
    pub application: Option<GraphAppIdentity>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphUserIdentity {
    pub id: String,
    pub display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphAppIdentity {
    pub id: String,
    pub display_name: Option<String>,
}

/// POST /chats/{id}/messages request body
#[derive(Debug, Serialize)]
pub struct SendMessageBody {
    pub body: SendMessageContent,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageContent {
    pub content_type: String,
    pub content: String,
}

/// GET /me response
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphMe {
    pub id: String,
    pub display_name: String,
    pub mail: Option<String>,
    pub user_principal_name: String,
}
