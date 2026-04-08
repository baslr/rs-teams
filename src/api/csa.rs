//! CSA (Chat Service Aggregator) API types and methods.
//!
//! Endpoint: GET /api/csa/{mt_region}/api/v3/teams/users/me
//! Token audience: https://chatsvcagg.teams.microsoft.com
//!
//! This endpoint returns chat folders, teams, chats, and a sync token
//! for delta synchronization.

use serde::{Deserialize, Deserializer};

use crate::api::client::GraphClient;
use crate::error::AppError;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Deserialize a `null` JSON value as `Default::default()` (e.g. `""` for String).
/// Combines with `#[serde(default)]` to handle both missing fields and explicit nulls.
fn deserialize_null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    let opt = Option::deserialize(deserializer)?;
    Ok(opt.unwrap_or_default())
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// Top-level response from /api/csa/.../api/v3/teams/users/me
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CsaUpdatesResponse {
    #[serde(default)]
    pub conversation_folders: Option<CsaConversationFolders>,
    #[serde(default)]
    pub metadata: Option<CsaMetadata>,
    #[serde(default)]
    pub chats: Vec<CsaChat>,
}

/// The conversationFolders section of the updates response
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CsaConversationFolders {
    pub folder_hierarchy_version: Option<u64>,
    #[serde(default)]
    pub conversation_folders: Vec<CsaFolder>,
    #[serde(default)]
    pub conversation_folder_order: Vec<String>,
    #[serde(default)]
    pub migrated_pinned_to_favorites: bool,
    #[serde(default)]
    pub is_partial_response: bool,
}

/// A single folder from the CSA response
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CsaFolder {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub folder_type: String,
    #[serde(default)]
    pub sort_type: String,
    #[serde(default)]
    pub is_expanded: bool,
    #[serde(default)]
    pub is_deleted: bool,
    #[serde(default)]
    pub version: u64,
    #[serde(default)]
    pub conversation_folder_items: Vec<CsaFolderItem>,
}

impl CsaFolder {
    /// Convenience: iterate over conversation IDs in this folder
    pub fn conversation_ids(&self) -> impl Iterator<Item = &str> {
        self.conversation_folder_items
            .iter()
            .map(|i| i.conversation_id.as_str())
    }
}

/// A conversation/item reference inside a folder
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CsaFolderItem {
    pub conversation_id: String,
    #[serde(default)]
    pub thread_type: String,
    #[serde(default)]
    pub item_type: String,
    #[serde(default)]
    pub created_time: u64,
    #[serde(default)]
    pub last_updated_time: u64,
}

/// Sync metadata
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CsaMetadata {
    pub sync_token: Option<String>,
    #[serde(default)]
    pub is_partial_data: bool,
    #[serde(default)]
    pub has_more_chats: bool,
}

// ---------------------------------------------------------------------------
// Chat types (from CSA full sync)
// ---------------------------------------------------------------------------

/// A chat from the CSA updates response `chats[]` array.
/// Contains complete chat metadata including members, read state, last message.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CsaChat {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub thread_type: String,
    #[serde(default)]
    pub is_read: bool,
    #[serde(default)]
    pub is_one_on_one: bool,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub is_disabled: bool,
    #[serde(default)]
    pub is_conversation_deleted: bool,
    #[serde(default)]
    pub members: Vec<CsaMember>,
    #[serde(default)]
    pub last_message: Option<CsaLastMessage>,
    #[serde(default)]
    pub consumption_horizon: Option<CsaConsumptionHorizon>,
}

/// A member in a CSA chat
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CsaMember {
    pub mri: String,
    #[serde(default)]
    pub object_id: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub is_muted: bool,
}

/// Last message in a CSA chat — contains sender display name inline
///
/// Note: many fields can be `null` in the real API, even though they look like
/// they should always be strings. We use `deserialize_null_default` to coerce
/// `null` → `""` so callers don't have to deal with Option everywhere.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CsaLastMessage {
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub id: String,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub im_display_name: String,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub content: String,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub compose_time: String,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub message_type: String,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub from: String,
}

/// Consumption horizon as structured object (CSA format)
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CsaConsumptionHorizon {
    #[serde(default)]
    pub original_arrival_time: u64,
    #[serde(default)]
    pub time_stamp: u64,
    #[serde(default)]
    pub client_message_id: String,
}

// ---------------------------------------------------------------------------
// API methods
// ---------------------------------------------------------------------------

impl GraphClient {
    /// Fetch conversation folders from the CSA endpoint.
    ///
    /// Uses the chatsvcagg token (audience: https://chatsvcagg.teams.microsoft.com).
    /// Returns folder definitions, folder display order, and chats.
    pub async fn get_folders(&self) -> Result<CsaUpdatesResponse, AppError> {
        self.get_csa(
            "api/v3/teams/users/me\
             ?isPrefetch=false\
             &enableMembershipSummary=true\
             &supportsAdditionalSystemGeneratedFolders=true\
             &supportsSliceItems=true\
             &enableEngageCommunities=false",
        )
        .await
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Real (anonymized) response from the CSA endpoint — delta sync (folders empty, order present)
    const DELTA_RESPONSE: &str = r#"{
        "conversationFolders": {
            "folderHierarchyVersion": 1775588983643,
            "conversationFolders": [],
            "conversationFolderOrder": [
                "tenant~user~QuickViews",
                "tenant~user~Favorites",
                "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
                "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb"
            ],
            "migratedPinnedToFavorites": true,
            "isPartialResponse": false
        },
        "teams": [],
        "chats": [],
        "metadata": {
            "syncToken": "eyJ0ZXN0IjoidG9rZW4ifQ==",
            "isPartialData": false,
            "hasMoreChats": false
        }
    }"#;

    /// Simulated full-sync response with folder details (matches real API format)
    const FULL_RESPONSE: &str = r#"{
        "conversationFolders": {
            "folderHierarchyVersion": 1775588983643,
            "conversationFolders": [
                {
                    "id": "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
                    "name": "MyFolder",
                    "folderType": "UserCreated",
                    "sortType": "UserDefinedCustomOrder",
                    "isExpanded": true,
                    "isDeleted": false,
                    "version": 1775588983643,
                    "conversationFolderItems": [
                        {"conversationId": "19:abc@thread.v2", "threadType": "chat", "createdTime": 100, "lastUpdatedTime": 200},
                        {"conversationId": "19:def@thread.v2", "threadType": "meeting", "createdTime": 300, "lastUpdatedTime": 400}
                    ]
                },
                {
                    "id": "tenant~user~Favorites",
                    "name": "Favorites",
                    "folderType": "Favorites",
                    "sortType": "UserDefinedCustomOrder",
                    "isExpanded": false,
                    "isDeleted": false,
                    "version": 123,
                    "conversationFolderItems": []
                }
            ],
            "conversationFolderOrder": [
                "tenant~user~Favorites",
                "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb"
            ],
            "migratedPinnedToFavorites": true,
            "isPartialResponse": false
        },
        "teams": [],
        "chats": [],
        "metadata": {
            "syncToken": "abc123",
            "isPartialData": false,
            "hasMoreChats": false
        }
    }"#;

    #[test]
    fn parse_delta_response_has_order_but_empty_folders() {
        let resp: CsaUpdatesResponse = serde_json::from_str(DELTA_RESPONSE).unwrap();
        let folders = resp.conversation_folders.unwrap();

        assert!(folders.conversation_folders.is_empty());
        assert_eq!(folders.conversation_folder_order.len(), 4);
        assert_eq!(folders.conversation_folder_order[0], "tenant~user~QuickViews");
        assert_eq!(folders.folder_hierarchy_version, Some(1775588983643));
        assert!(folders.migrated_pinned_to_favorites);
        assert!(!folders.is_partial_response);
    }

    #[test]
    fn parse_delta_response_has_metadata() {
        let resp: CsaUpdatesResponse = serde_json::from_str(DELTA_RESPONSE).unwrap();
        let meta = resp.metadata.unwrap();

        assert_eq!(meta.sync_token.as_deref(), Some("eyJ0ZXN0IjoidG9rZW4ifQ=="));
        assert!(!meta.is_partial_data);
        assert!(!meta.has_more_chats);
    }

    #[test]
    fn parse_full_response_has_folder_details() {
        let resp: CsaUpdatesResponse = serde_json::from_str(FULL_RESPONSE).unwrap();
        let folders = resp.conversation_folders.unwrap();

        assert_eq!(folders.conversation_folders.len(), 2);

        let folder = &folders.conversation_folders[0];
        assert_eq!(folder.id, "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb");
        assert_eq!(folder.name, "MyFolder");
        assert_eq!(folder.folder_type, "UserCreated");
        assert_eq!(folder.sort_type, "UserDefinedCustomOrder");
        assert!(folder.is_expanded);
        assert!(!folder.is_deleted);
        assert_eq!(folder.version, 1775588983643);
        assert_eq!(folder.conversation_folder_items.len(), 2);
        assert_eq!(folder.conversation_folder_items[0].conversation_id, "19:abc@thread.v2");
        assert_eq!(folder.conversation_folder_items[0].thread_type, "chat");
        assert_eq!(folder.conversation_folder_items[0].created_time, 100);
        assert_eq!(folder.conversation_folder_items[1].thread_type, "meeting");
        assert_eq!(folder.conversation_folder_items[1].last_updated_time, 400);

        let favs = &folders.conversation_folders[1];
        assert_eq!(favs.folder_type, "Favorites");
        assert!(favs.conversation_folder_items.is_empty());
    }

    #[test]
    fn parse_full_response_has_order() {
        let resp: CsaUpdatesResponse = serde_json::from_str(FULL_RESPONSE).unwrap();
        let folders = resp.conversation_folders.unwrap();

        assert_eq!(folders.conversation_folder_order.len(), 2);
        assert_eq!(folders.conversation_folder_order[0], "tenant~user~Favorites");
        assert_eq!(folders.conversation_folder_order[1], "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb");
    }

    #[test]
    fn parse_empty_response() {
        let resp: CsaUpdatesResponse = serde_json::from_str("{}").unwrap();
        assert!(resp.conversation_folders.is_none());
        assert!(resp.metadata.is_none());
    }

    #[test]
    fn parse_folder_with_missing_optional_fields() {
        let json = r#"{
            "conversationFolders": {
                "conversationFolders": [
                    {"id": "test-id"}
                ],
                "conversationFolderOrder": []
            }
        }"#;
        let resp: CsaUpdatesResponse = serde_json::from_str(json).unwrap();
        let folders = resp.conversation_folders.unwrap();
        let f = &folders.conversation_folders[0];

        assert_eq!(f.id, "test-id");
        assert_eq!(f.name, "");
        assert_eq!(f.folder_type, "");
        assert!(!f.is_expanded);
        assert!(!f.is_deleted);
        assert_eq!(f.version, 0);
        assert!(f.conversation_folder_items.is_empty());
    }

    // ── CsaChat deserialization ───────────────────────────────────

    #[test]
    fn parse_csa_chat_full() {
        let json = r#"{
            "id": "19:abc_def@unq.gbl.spaces",
            "title": "Project Alpha",
            "threadType": "chat",
            "isRead": false,
            "isOneOnOne": false,
            "hidden": false,
            "isDisabled": false,
            "isConversationDeleted": false,
            "members": [
                {"mri": "8:orgid:aaa", "objectId": "aaa", "role": "Admin", "isMuted": false},
                {"mri": "8:orgid:bbb", "objectId": "bbb", "role": "User", "isMuted": true}
            ],
            "lastMessage": {
                "id": "1775589540601",
                "imDisplayName": "Max Mustermann",
                "content": "<p>Hello!</p>",
                "composeTime": "2026-04-07T19:19:00Z",
                "messageType": "RichText/Html",
                "from": "8:orgid:aaa"
            },
            "consumptionHorizon": {
                "originalArrivalTime": 1775589000000,
                "timeStamp": 1775589000000,
                "clientMessageId": "msg-123"
            }
        }"#;
        let chat: CsaChat = serde_json::from_str(json).unwrap();

        assert_eq!(chat.id, "19:abc_def@unq.gbl.spaces");
        assert_eq!(chat.title.as_deref(), Some("Project Alpha"));
        assert_eq!(chat.thread_type, "chat");
        assert!(!chat.is_read);
        assert!(!chat.is_one_on_one);
        assert!(!chat.hidden);
        assert_eq!(chat.members.len(), 2);
        assert_eq!(chat.members[0].mri, "8:orgid:aaa");
        assert_eq!(chat.members[0].role, "Admin");
        assert!(chat.members[1].is_muted);

        let lm = chat.last_message.unwrap();
        assert_eq!(lm.id, "1775589540601");
        assert_eq!(lm.im_display_name, "Max Mustermann");
        assert_eq!(lm.content, "<p>Hello!</p>");
        assert_eq!(lm.from, "8:orgid:aaa");

        let ch = chat.consumption_horizon.unwrap();
        assert_eq!(ch.original_arrival_time, 1775589000000);
        assert_eq!(ch.client_message_id, "msg-123");
    }

    #[test]
    fn parse_csa_chat_minimal() {
        let json = r#"{"id": "19:test@thread.v2"}"#;
        let chat: CsaChat = serde_json::from_str(json).unwrap();

        assert_eq!(chat.id, "19:test@thread.v2");
        assert!(chat.title.is_none());
        assert_eq!(chat.thread_type, "");
        assert!(!chat.is_read); // serde default = false
        assert!(!chat.is_one_on_one);
        assert!(!chat.hidden);
        assert!(chat.members.is_empty());
        assert!(chat.last_message.is_none());
        assert!(chat.consumption_horizon.is_none());
    }

    #[test]
    fn parse_response_with_chats() {
        let json = r#"{
            "chats": [
                {"id": "19:aaa@thread.v2", "title": "Chat A", "threadType": "chat", "isRead": true},
                {"id": "19:bbb@thread.v2", "threadType": "meeting", "isRead": false}
            ],
            "metadata": {"syncToken": "tok", "isPartialData": false, "hasMoreChats": false}
        }"#;
        let resp: CsaUpdatesResponse = serde_json::from_str(json).unwrap();

        assert_eq!(resp.chats.len(), 2);
        assert_eq!(resp.chats[0].title.as_deref(), Some("Chat A"));
        assert!(resp.chats[0].is_read);
        assert_eq!(resp.chats[1].thread_type, "meeting");
        assert!(!resp.chats[1].is_read);
    }

    #[test]
    fn parse_response_without_chats_defaults_empty() {
        let json = r#"{"metadata": {"syncToken": "tok"}}"#;
        let resp: CsaUpdatesResponse = serde_json::from_str(json).unwrap();
        assert!(resp.chats.is_empty());
    }

    #[test]
    fn parse_real_api_response_file() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/api_explore_csa_me.json");
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => {
                eprintln!("Skipping test: {path} not found");
                return;
            }
        };

        match serde_json::from_str::<CsaUpdatesResponse>(&content) {
            Ok(resp) => {
                if let Some(folders) = &resp.conversation_folders {
                    assert!(!folders.conversation_folders.is_empty(), "Expected folders");
                }
                assert!(!resp.chats.is_empty(), "Expected chats");
            }
            Err(e) => {
                // Show detailed context around the error
                let line = e.line();
                let col = e.column();
                let lines: Vec<&str> = content.lines().collect();
                let error_line = if line > 0 && line <= lines.len() {
                    lines[line - 1]
                } else {
                    ""
                };
                let start = col.saturating_sub(60);
                let end = (col + 60).min(error_line.len());
                let context = &error_line[start..end];
                panic!(
                    "Parse error at line {line} col {col}: {e}\n  context: ...{context}..."
                );
            }
        }
    }
}
