use crate::api::client::GraphClient;
use crate::error::AppError;

const GRAPH_BASE: &str = "https://graph.microsoft.com/v1.0";

/// Response types — raw JSON for now, until we know the exact format
pub type ChatList = serde_json::Value;
pub type MessageList = serde_json::Value;

impl GraphClient {
    /// Get the current user's profile via Graph API
    pub async fn me(&self) -> Result<serde_json::Value, AppError> {
        self.get_graph(&format!("{GRAPH_BASE}/me")).await
    }

    /// Load ALL chats by paging through the chatsvc conversations endpoint.
    /// Returns a single JSON value with all conversations merged into one array.
    pub async fn list_all_chats(&self) -> Result<serde_json::Value, AppError> {
        let mut all_conversations: Vec<serde_json::Value> = Vec::new();

        // First page: pageSize=200
        let first: serde_json::Value = self.get_chatsvc(
            "v1/users/ME/conversations?view=msnp24Equivalent&pageSize=200"
        ).await?;

        if let Some(convs) = first.get("conversations").and_then(|v| v.as_array()) {
            all_conversations.extend(convs.iter().cloned());
        }

        // Follow backwardLink for more pages
        let mut backward_link = first
            .get("_metadata")
            .and_then(|m| m.get("backwardLink"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        while let Some(url) = backward_link.take() {
            tracing::debug!("Paging chats: {} loaded so far", all_conversations.len());
            let page: serde_json::Value = self.get_chatsvc_absolute(&url).await?;

            if let Some(convs) = page.get("conversations").and_then(|v| v.as_array()) {
                if convs.is_empty() {
                    break;
                }
                all_conversations.extend(convs.iter().cloned());
            } else {
                break;
            }

            backward_link = page
                .get("_metadata")
                .and_then(|m| m.get("backwardLink"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());
        }

        tracing::info!("Loaded all chats: {} total conversations", all_conversations.len());

        Ok(serde_json::json!({
            "conversations": all_conversations
        }))
    }

    /// List messages in a specific chat
    pub async fn list_messages(
        &self,
        conversation_id: &str,
    ) -> Result<serde_json::Value, AppError> {
        let encoded_id = urlencoding::encode(conversation_id);
        let path = format!(
            "v1/users/ME/conversations/{encoded_id}/messages?view=msnp24Equivalent&pageSize=50"
        );
        self.get_chatsvc(&path).await
    }

    /// Send a message to a conversation
    pub async fn send_message(
        &self,
        conversation_id: &str,
        content: &str,
        display_name: &str,
    ) -> Result<serde_json::Value, AppError> {
        let encoded_id = urlencoding::encode(conversation_id);
        let path = format!(
            "v1/users/ME/conversations/{encoded_id}/messages"
        );
        let body = serde_json::json!({
            "content": content,
            "messagetype": "RichText",
            "contenttype": "text",
            "imdisplayname": display_name,
            "clientmessageid": format!("{}", chrono::Utc::now().timestamp_millis())
        });
        self.post_chatsvc(&path, &body).await
    }

    /// Fetch user profiles (the /api/mt/{region}/beta/users/fetch endpoint)
    pub async fn fetch_users(
        &self,
        user_ids: &[String],
    ) -> Result<serde_json::Value, AppError> {
        let body: Vec<&str> = user_ids.iter().map(|s| s.as_str()).collect();
        self.post_mt(
            "beta/users/fetch?isMailAddress=false&enableGuest=true&skypeTeamsInfo=true",
            &body,
        ).await
    }

    /// Fetch federated (external org) user profiles
    /// Uses /api/mt/{region}/beta/users/fetchFederated
    pub async fn fetch_federated(
        &self,
        user_ids: &[String],
    ) -> Result<serde_json::Value, AppError> {
        let body: Vec<&str> = user_ids.iter().map(|s| s.as_str()).collect();
        self.post_mt(
            "beta/users/fetchFederated?edEnabled=false&includeDisabledAccounts=true",
            &body,
        ).await
    }
}
