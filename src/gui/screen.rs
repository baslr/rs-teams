use std::collections::{HashMap, HashSet};

use iced::widget::{center, column, row, scrollable, text, container, button, tooltip, Column};
use iced::{Color, Element, Font, Length, Subscription, Task};

use crate::api::client::GraphClient;
use crate::api::csa::{CsaChat, CsaFolder, CsaFolderConversation, CsaLastMessage, CsaMember};
use crate::api::csa::CsaUpdatesResponse;
use crate::error::AppError;
use crate::gui::style::TeamsDark;
use crate::models::chat::strip_html;

/// Helper: wrap any widget in a tooltip with a monospaced 3-char ID label
fn tid<'a, M: 'a>(id: &'a str, widget: Element<'a, M>) -> Element<'a, M> {
    tooltip(
        widget,
        container(
            text(id).size(11).font(Font::MONOSPACE).color(Color::WHITE)
        )
        .padding([2, 6])
        .style(|_theme| container::Style {
            background: Some(iced::Background::Color(Color::from_rgb(0.1, 0.1, 0.1))),
            border: iced::Border {
                color: Color::from_rgb(0.5, 0.5, 0.5),
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        }),
        tooltip::Position::Top,
    )
    .into()
}

/// A parsed chat message ready for display
#[derive(Debug, Clone)]
pub struct ParsedMessage {
    pub sender_name: String,
    pub sender_id: String,
    pub content: String,
    pub timestamp: String,
    pub is_from_me: bool,
}

/// Visual properties for rendering a single chat row — testable without iced widgets.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatDisplayProps {
    pub text_color: Color,
    pub bold: bool,
    pub show_indicator: bool,
    pub indicator_color: Color,
    pub badge_text: Option<String>,
}

/// Compute display properties for a chat entry. Pure function, fully testable.
pub fn chat_display_props(chat: &ChatEntry) -> ChatDisplayProps {
    let has_unread = chat.unread_count > 0;
    ChatDisplayProps {
        text_color: Color::WHITE,
        bold: has_unread,
        show_indicator: has_unread,
        indicator_color: TeamsDark::ACCENT,
        badge_text: if has_unread {
            Some(if chat.unread_count >= 10 {
                "9+".to_string()
            } else {
                chat.unread_count.to_string()
            })
        } else {
            None
        },
    }
}

/// A sidebar section — either a named folder or the catch-all "Sonstige"
#[derive(Debug, Clone, PartialEq)]
pub struct SidebarSection {
    pub folder_id: String,
    pub name: String,
    pub chat_indices: Vec<usize>,
    pub is_system: bool,
}

/// Group chats into sidebar sections based on folder assignments.
/// Chats can appear in multiple folders (e.g. Favorites AND a custom folder).
/// "Sonstige" only contains chats not assigned to any folder.
/// Pure function, fully testable.
pub fn build_sidebar_sections(
    chats: &[ChatEntry],
    folders: &[CsaFolder],
    folder_order: &[String],
) -> Vec<SidebarSection> {
    let mut sections = Vec::new();
    let mut ever_assigned: HashSet<usize> = HashSet::new();

    for folder_id in folder_order {
        let Some(folder) = folders.iter().find(|f| &f.id == folder_id) else {
            continue;
        };
        if folder.is_deleted {
            continue;
        }

        let is_system = matches!(
            folder.folder_type.as_str(),
            "Favorites" | "QuickViews"
        );

        let conv_ids: HashSet<&str> = folder
            .conversations
            .iter()
            .map(|c| c.id.as_str())
            .collect();

        let mut chat_indices: Vec<usize> = Vec::new();
        for (i, c) in chats.iter().enumerate() {
            if conv_ids.contains(c.id.as_str()) {
                ever_assigned.insert(i);
                chat_indices.push(i);
            }
        }

        sections.push(SidebarSection {
            folder_id: folder.id.clone(),
            name: folder.name.clone(),
            chat_indices,
            is_system,
        });
    }

    // Collect chats not assigned to ANY folder into "Sonstige"
    let other_indices: Vec<usize> = (0..chats.len())
        .filter(|i| !ever_assigned.contains(i))
        .collect();

    if !other_indices.is_empty() || sections.is_empty() {
        sections.push(SidebarSection {
            folder_id: "___other___".to_string(),
            name: "Sonstige".to_string(),
            chat_indices: other_indices,
            is_system: false,
        });
    }

    sections
}

/// Convert CSA chat objects to ChatEntry list for the sidebar.
/// Pure function, fully testable.
pub fn parse_csa_chats(chats: &[CsaChat], my_user_id: &str) -> Vec<ChatEntry> {
    let my_bare_id = my_user_id
        .strip_prefix("8:orgid:")
        .unwrap_or(my_user_id);

    let mut entries = Vec::new();

    for chat in chats {
        // Skip hidden, deleted, disabled chats
        if chat.hidden || chat.is_conversation_deleted || chat.is_disabled {
            continue;
        }

        // Build display name
        let name = if let Some(title) = &chat.title {
            if !title.is_empty() {
                title.clone()
            } else {
                resolve_csa_chat_name(chat, my_bare_id)
            }
        } else {
            resolve_csa_chat_name(chat, my_bare_id)
        };

        // Thread type prefix
        let display_name = match chat.thread_type.as_str() {
            "chat" => format!("💬 {name}"),
            "meeting" => format!("📅 {name}"),
            "topic" => format!("📋 {name}"),
            _ => name,
        };

        let unread_count = if chat.is_read { 0 } else { 1 };

        let read_horizon_id = chat
            .consumption_horizon
            .as_ref()
            .map(|ch| ch.original_arrival_time)
            .unwrap_or(0);

        entries.push(ChatEntry {
            id: chat.id.clone(),
            display_name,
            unread_count,
            read_horizon_id,
        });
    }

    entries
}

/// Resolve display name for a CSA chat that has no title.
fn resolve_csa_chat_name(chat: &CsaChat, my_bare_id: &str) -> String {
    // For 1:1 chats: try to get other member's name from lastMessage
    if chat.is_one_on_one {
        if let Some(lm) = &chat.last_message {
            if !lm.im_display_name.is_empty() {
                // If the last message is from the other person, use their name
                let from_bare = lm.from
                    .strip_prefix("8:orgid:")
                    .unwrap_or(&lm.from);
                if from_bare != my_bare_id {
                    return lm.im_display_name.clone();
                }
            }
        }
        // Fallback: find the other member's MRI
        for member in &chat.members {
            let member_bare = member.mri
                .strip_prefix("8:orgid:")
                .unwrap_or(&member.mri);
            if member_bare != my_bare_id {
                // Use object_id as short identifier
                if member.object_id.len() > 8 {
                    return format!("User {}", &member.object_id[..8]);
                } else {
                    return format!("User {}", member.object_id);
                }
            }
        }
    }

    // Group chats without title: use member names
    let other_names: Vec<&str> = chat.members.iter()
        .filter(|m| {
            let bare = m.mri.strip_prefix("8:orgid:").unwrap_or(&m.mri);
            bare != my_bare_id
        })
        .map(|m| {
            if !m.object_id.is_empty() {
                m.object_id.as_str()
            } else {
                m.mri.as_str()
            }
        })
        .take(3)
        .collect();

    if !other_names.is_empty() {
        other_names.join(", ")
    } else {
        chat.id.chars().take(30).collect()
    }
}

/// Fill in placeholder ChatEntry items for conversations referenced by folders
/// but missing from the chat list. This ensures folder groups are never empty
/// when the folder has conversation IDs.
pub fn fill_missing_folder_chats(
    chats: &mut Vec<ChatEntry>,
    folders: &[CsaFolder],
) {
    let existing_ids: HashSet<String> = chats.iter().map(|c| c.id.clone()).collect();

    for folder in folders {
        if folder.is_deleted {
            continue;
        }
        for conv in &folder.conversations {
            if !existing_ids.contains(&conv.id) {
                let prefix = match conv.thread_type.as_str() {
                    "chat" => "💬 ",
                    "meeting" => "📅 ",
                    "topic" => "📋 ",
                    "space" => "📌 ",
                    _ => "💬 ",
                };
                chats.push(ChatEntry {
                    id: conv.id.clone(),
                    display_name: format!("{prefix}{}", short_id(&conv.id)),
                    unread_count: 0,
                    read_horizon_id: 0,
                });
            }
        }
    }
}

// ── Custom chat names persistence ──────────────────────────────────────

const CUSTOM_NAMES_PATH: &str = "data/chat_names.json";

/// Load custom chat names from JSON file. Returns empty map on any error.
pub fn load_custom_names() -> HashMap<String, String> {
    std::fs::read_to_string(CUSTOM_NAMES_PATH)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Save custom chat names to JSON file.
pub fn save_custom_names(names: &HashMap<String, String>) {
    if let Some(parent) = std::path::Path::new(CUSTOM_NAMES_PATH).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(names) {
        let _ = std::fs::write(CUSTOM_NAMES_PATH, json);
    }
}

/// Return the custom name if set, otherwise the original name. Pure function.
pub fn resolve_display_name(
    chat_id: &str,
    original: &str,
    custom_names: &HashMap<String, String>,
) -> String {
    if let Some(custom) = custom_names.get(chat_id) {
        if !custom.is_empty() {
            return custom.clone();
        }
    }
    original.to_string()
}

/// Parse a fetchFederated API response into a map of MRI → displayName.
/// Used for resolving external organization user names.
pub fn parse_federated_profiles(json: &str) -> HashMap<String, String> {
    let mut result = HashMap::new();
    let val: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return result,
    };
    if let Some(users) = val.get("value").and_then(|v| v.as_array()) {
        for user in users {
            let mri = user.get("mri").and_then(|v| v.as_str()).unwrap_or("");
            let name = user.get("displayName").and_then(|v| v.as_str()).unwrap_or("");
            if !mri.is_empty() && !name.is_empty() {
                result.insert(mri.to_string(), name.to_string());
            }
        }
    }
    result
}

/// Extract a short display label from a conversation ID
fn short_id(id: &str) -> String {
    // "19:abc_def@unq.gbl.spaces" → "abc_def" (truncated)
    let inner = id
        .strip_prefix("19:")
        .unwrap_or(id)
        .split('@')
        .next()
        .unwrap_or(id);
    if inner.len() > 20 {
        format!("{}…", &inner[..20])
    } else {
        inner.to_string()
    }
}

/// A chat entry in the sidebar
#[derive(Debug, Clone)]
pub struct ChatEntry {
    pub id: String,
    pub display_name: String,
    pub unread_count: u32,
    /// consumptionhorizon read ID — messages with id > this are unread
    pub read_horizon_id: u64,
}

pub struct MainScreen {
    client: GraphClient,
    my_display_name: String,
    my_user_id: String,
    /// Raw JSON of the chats response for debugging
    chats_json: String,
    /// Parsed chat list
    chats: Vec<ChatEntry>,
    /// Folder definitions from CSA API
    folders: Vec<CsaFolder>,
    /// Folder display order (folder IDs)
    folder_order: Vec<String>,
    /// Set of folder IDs that are collapsed in the sidebar
    collapsed_folders: HashSet<String>,
    /// Currently selected chat
    selected_chat_id: Option<String>,
    selected_chat_name: Option<String>,
    /// Parsed messages for display
    messages: Vec<ParsedMessage>,
    /// URL to load older messages (from _metadata.backwardLink)
    backward_link: Option<String>,
    /// True while loading older messages
    loading_older: bool,
    /// Error message if message loading failed
    messages_error: Option<String>,
    input_value: String,
    loading: bool,
    /// Custom chat names (chat_id → custom name), persisted to JSON
    custom_chat_names: HashMap<String, String>,
    /// True when the H01 header is in rename mode
    renaming_chat: bool,
    /// Current text in the rename input
    rename_value: String,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// CSA full sync loaded: chats + folders in one response
    CsaSyncLoaded(Result<CsaUpdatesResponse, String>),
    /// Fallback: chatsvc chat list loaded
    ChatsLoaded(Result<String, String>),
    ChatSelected(String, String),
    MessagesLoaded(Result<String, String>),
    InputChanged(String),
    SendPressed,
    MessageSent(Result<String, String>),
    ProfileLoaded(Result<String, String>),
    /// Federated (external org) user profiles loaded
    FederatedProfileLoaded(Result<String, String>),
    OlderMessagesLoaded(Result<String, String>),
    PollMessagesLoaded(Result<String, String>),
    PollTick,
    ScrollChanged(scrollable::Viewport),
    /// (chat_id, unread_count) for a single chat
    UnreadCountLoaded(String, u32),
    /// Folders loaded from CSA API: (folders, folder_order)
    FoldersLoaded(Result<(Vec<CsaFolder>, Vec<String>), String>),
    /// Toggle collapse state for a folder
    FolderToggle(String),
    /// Start renaming the current chat (click on H01)
    RenameStart,
    /// Rename input text changed
    RenameChanged(String),
    /// Rename submitted (Enter)
    RenameSubmit,
}

impl MainScreen {
    pub fn new(
        client: GraphClient,
        display_name: String,
        user_id: String,
        initial_folders: Vec<CsaFolder>,
        initial_folder_order: Vec<String>,
    ) -> (Self, Task<Message>) {
        let c = client.clone();

        // Single CSA full sync: returns chats + folders in one request
        let task = Task::perform(
            async move {
                let resp: CsaUpdatesResponse = c.get_folders().await?;
                Ok::<_, AppError>(resp)
            },
            |r| Message::CsaSyncLoaded(r.map_err(|e| e.to_string())),
        );

        (
            Self {
                client,
                my_display_name: display_name,
                my_user_id: user_id,
                chats_json: "Loading...".into(),
                chats: vec![],
                folders: initial_folders,
                folder_order: initial_folder_order,
                collapsed_folders: HashSet::new(),
                selected_chat_id: None,
                selected_chat_name: None,
                messages: vec![],
                backward_link: None,
                loading_older: false,
                messages_error: None,
                input_value: String::new(),
                loading: true,
                custom_chat_names: load_custom_names(),
                renaming_chat: false,
                rename_value: String::new(),
            },
            task,
        )
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::CsaSyncLoaded(Ok(resp)) => {
                self.loading = false;

                // Extract folders
                if let Some(cf) = resp.conversation_folders {
                    tracing::info!(
                        "CSA folders: {} folders in response, {} in order",
                        cf.conversation_folders.len(),
                        cf.conversation_folder_order.len()
                    );
                    if !cf.conversation_folders.is_empty() {
                        self.folders = cf.conversation_folders;
                    }
                    if !cf.conversation_folder_order.is_empty() {
                        self.folder_order = cf.conversation_folder_order;
                    }
                }

                // Log folder details
                for folder in &self.folders {
                    tracing::info!(
                        "  Folder '{}' ({}): {} conversations, type={}, deleted={}",
                        folder.name,
                        folder.id,
                        folder.conversations.len(),
                        folder.folder_type,
                        folder.is_deleted
                    );
                    for conv in &folder.conversations {
                        tracing::debug!("    conv: {} (type={})", conv.id, conv.thread_type);
                    }
                }

                // Extract chats from CSA response
                tracing::info!("CSA full sync: {} chats in response", resp.chats.len());
                if !resp.chats.is_empty() {
                    self.chats = parse_csa_chats(&resp.chats, &self.my_user_id);
                    tracing::info!("CSA parsed: {} chats (after filtering hidden/deleted)", self.chats.len());
                    fill_missing_folder_chats(&mut self.chats, &self.folders);
                    tracing::info!("After fill_missing: {} chats total", self.chats.len());
                    self.chats_json = format!("{} chats from CSA sync", self.chats.len());
                } else {
                    tracing::warn!("CSA returned 0 chats — will rely on chatsvc paging");
                    fill_missing_folder_chats(&mut self.chats, &self.folders);
                    tracing::info!("After fill_missing (no CSA chats): {} placeholder chats", self.chats.len());
                }

                // Log folder→chat matching
                for folder in &self.folders {
                    let matched: Vec<&str> = folder.conversations.iter()
                        .filter(|c| self.chats.iter().any(|ch| ch.id == c.id))
                        .map(|c| c.id.as_str())
                        .collect();
                    let missing: Vec<&str> = folder.conversations.iter()
                        .filter(|c| !self.chats.iter().any(|ch| ch.id == c.id))
                        .map(|c| c.id.as_str())
                        .collect();
                    tracing::info!(
                        "  Folder '{}': {}/{} matched, {} missing",
                        folder.name,
                        matched.len(),
                        folder.conversations.len(),
                        missing.len()
                    );
                    for m in &missing {
                        tracing::warn!("    MISSING in chat list: {}", m);
                    }
                }

                // Always load all chats via chatsvc paging to fill in missing metadata
                let c = self.client.clone();
                Task::perform(
                    async move {
                        let resp: serde_json::Value = c.list_all_chats().await?;
                        Ok::<_, AppError>(serde_json::to_string_pretty(&resp).unwrap_or_default())
                    },
                    |r| Message::ChatsLoaded(r.map_err(|e| e.to_string())),
                )
            }
            Message::CsaSyncLoaded(Err(e)) => {
                tracing::warn!("CSA sync failed, falling back to chatsvc paging: {e}");
                // Fallback to chatsvc list_all_chats (paged!)
                let c = self.client.clone();
                Task::perform(
                    async move {
                        let resp: serde_json::Value = c.list_all_chats().await?;
                        Ok::<_, AppError>(serde_json::to_string_pretty(&resp).unwrap_or_default())
                    },
                    |r| Message::ChatsLoaded(r.map_err(|e| e.to_string())),
                )
            }
            Message::ChatsLoaded(Ok(json)) => {
                self.chats_json = json.clone();
                self.loading = false;

                // Parse chatsvc results and merge into existing chat list
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&json) {
                    let incoming = parse_chats(&val, &self.my_user_id);
                    tracing::info!("ChatsLoaded: {} incoming chats from chatsvc paging", incoming.len());

                    if self.chats.is_empty() {
                        // No CSA chats — use chatsvc as primary source
                        self.chats = incoming;
                        tracing::info!("ChatsLoaded: used as primary source ({} chats)", self.chats.len());
                    } else {
                        // Merge into existing CSA-sourced chats
                        let before = self.chats.len();
                        merge_chatsvc_into_existing(&mut self.chats, incoming);
                        tracing::info!(
                            "ChatsLoaded: merged — {} before, {} after ({} added)",
                            before,
                            self.chats.len(),
                            self.chats.len() - before
                        );
                    }

                    // Fill in placeholders for folder-referenced chats still missing
                    let unread_chats: Vec<&str> = self.chats.iter()
                        .filter(|c| c.unread_count > 0)
                        .map(|c| c.display_name.as_str())
                        .collect();
                    tracing::info!(
                        "ChatsLoaded: {} unread chats detected: {:?}",
                        unread_chats.len(),
                        unread_chats
                    );

                    let before_fill = self.chats.len();
                    fill_missing_folder_chats(&mut self.chats, &self.folders);
                    if self.chats.len() > before_fill {
                        tracing::info!(
                            "ChatsLoaded: fill_missing added {} placeholders ({} total)",
                            self.chats.len() - before_fill,
                            self.chats.len()
                        );
                    }

                    // Log folder matching AFTER merge
                    for folder in &self.folders {
                        let matched: Vec<&str> = folder.conversations.iter()
                            .filter(|c| self.chats.iter().any(|ch| ch.id == c.id))
                            .map(|c| c.id.as_str())
                            .collect();
                        let missing: Vec<&str> = folder.conversations.iter()
                            .filter(|c| !self.chats.iter().any(|ch| ch.id == c.id))
                            .map(|c| c.id.as_str())
                            .collect();
                        if !missing.is_empty() {
                            tracing::warn!(
                                "After merge — Folder '{}': {}/{} matched, {} STILL MISSING: {:?}",
                                folder.name,
                                matched.len(),
                                folder.conversations.len(),
                                missing.len(),
                                missing
                            );
                        } else {
                            tracing::info!(
                                "After merge — Folder '{}': {}/{} all matched",
                                folder.name,
                                folder.conversations.len(),
                                folder.conversations.len()
                            );
                        }
                    }
                }

                // Write to debug log
                let _ = std::fs::write(
                    "chats_response.json",
                    &json,
                );

                // Collect tasks to run in parallel
                let mut tasks: Vec<Task<Message>> = Vec::new();

                // Resolve user names for 1:1 chats that show as "User ..."
                let user_ids_to_resolve: Vec<String> = self.chats.iter()
                    .filter(|c| c.display_name.starts_with("💬 User "))
                    .filter_map(|c| {
                        // Extract the other user's MRI from conv ID
                        let inner = c.id.strip_prefix("19:")?.split('@').next()?;
                        let my_bare = self.my_user_id.strip_prefix("8:orgid:").unwrap_or(&self.my_user_id);
                        let parts: Vec<&str> = inner.split('_').collect();
                        if parts.len() == 2 {
                            let other = if parts[0].starts_with(my_bare) { parts[1] } else { parts[0] };
                            Some(format!("8:orgid:{other}"))
                        } else {
                            None
                        }
                    })
                    .collect();

                tracing::info!(
                    "Name resolution: {} chats still show 'User ...', resolving {} MRIs via users/fetch",
                    self.chats.iter().filter(|c| c.display_name.starts_with("💬 User ")).count(),
                    user_ids_to_resolve.len()
                );
                for mri in &user_ids_to_resolve {
                    tracing::debug!("  resolving: {mri}");
                }

                if !user_ids_to_resolve.is_empty() {
                    let c = self.client.clone();
                    let ids_for_log = user_ids_to_resolve.clone();
                    let user_ids_to_resolve_fed = user_ids_to_resolve.clone();
                    tasks.push(Task::perform(
                        async move {
                            let resp: serde_json::Value = c.fetch_users(&user_ids_to_resolve).await?;
                            // Log the raw response
                            let _ = std::fs::write(
                                "users_fetch_response.json",
                                serde_json::to_string_pretty(&resp).unwrap_or_default(),
                            );
                            tracing::info!("users/fetch: sent {} MRIs, response keys: {:?}",
                                ids_for_log.len(),
                                resp.as_object().map(|o| o.keys().collect::<Vec<_>>())
                            );
                            if let Some(users) = resp.get("value").and_then(|v| v.as_array()) {
                                tracing::info!("users/fetch: {} users returned", users.len());
                                for u in users {
                                    let mri = u.get("mri").and_then(|v| v.as_str()).unwrap_or("?");
                                    let name = u.get("displayName").and_then(|v| v.as_str()).unwrap_or("?");
                                    tracing::info!("  resolved: {} -> {}", mri, name);
                                }
                                // Log which MRIs were NOT returned
                                let returned_mris: HashSet<&str> = users.iter()
                                    .filter_map(|u| u.get("mri").and_then(|v| v.as_str()))
                                    .collect();
                                for mri in &ids_for_log {
                                    if !returned_mris.contains(mri.as_str()) {
                                        tracing::warn!("  NOT RESOLVED: {mri}");
                                    }
                                }
                            } else {
                                tracing::warn!("users/fetch: no 'value' array in response");
                            }
                            Ok::<_, AppError>(serde_json::to_string(&resp).unwrap_or_default())
                        },
                        |r| Message::ProfileLoaded(r.map_err(|e| e.to_string())),
                    ));

                    // Also fire fetchFederated in parallel for external org users
                    let c2 = self.client.clone();
                    let federated_ids = user_ids_to_resolve_fed.clone();
                    tasks.push(Task::perform(
                        async move {
                            let resp: serde_json::Value = c2.fetch_federated(&federated_ids).await?;
                            tracing::info!("users/fetchFederated: sent {} MRIs, got {} results",
                                federated_ids.len(),
                                resp.get("value").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0)
                            );
                            Ok::<_, AppError>(serde_json::to_string(&resp).unwrap_or_default())
                        },
                        |r| Message::FederatedProfileLoaded(r.map_err(|e| e.to_string())),
                    ));
                }

                // Load unread counts: fetch messages for each unread chat
                for chat in &self.chats {
                    if chat.unread_count > 0 {
                        let c = self.client.clone();
                        let chat_id = chat.id.clone();
                        let chat_id2 = chat.id.clone();
                        let horizon = chat.read_horizon_id;
                        tasks.push(Task::perform(
                            async move {
                                let resp: serde_json::Value = c.list_messages(&chat_id).await?;
                                // Count messages with id > read_horizon_id
                                let count = resp.get("messages")
                                    .and_then(|v| v.as_array())
                                    .map(|msgs| {
                                        msgs.iter().filter(|m| {
                                            let msg_type = m.get("type").and_then(|v| v.as_str()).unwrap_or("");
                                            if msg_type != "Message" { return false; }
                                            let mid: u64 = m.get("id")
                                                .and_then(|v| v.as_str())
                                                .and_then(|s| s.parse().ok())
                                                .unwrap_or(0);
                                            mid > horizon
                                        }).count() as u32
                                    })
                                    .unwrap_or(0);
                                Ok::<_, AppError>((chat_id, count))
                            },
                            move |r| match r {
                                Ok((id, count)) => Message::UnreadCountLoaded(id, count),
                                Err(e) => {
                                    tracing::debug!("Unread count failed for {chat_id2}: {e}");
                                    Message::UnreadCountLoaded(chat_id2, 0)
                                }
                            },
                        ));
                    }
                }

                if tasks.is_empty() {
                    Task::none()
                } else {
                    Task::batch(tasks)
                }
            }
            Message::ChatsLoaded(Err(e)) => {
                self.chats_json = format!("Error: {e}");
                self.loading = false;
                Task::none()
            }
            Message::ChatSelected(id, name) => {
                self.selected_chat_id = Some(id.clone());
                self.selected_chat_name = Some(name);
                self.messages = vec![];
                self.backward_link = None;
                self.loading_older = false;
                self.renaming_chat = false;
                self.messages_error = None;

                let c = self.client.clone();
                Task::perform(
                    async move {
                        let resp: serde_json::Value = c.list_messages(&id).await?;
                        let _ = std::fs::write(
                            "messages_response.json",
                            serde_json::to_string_pretty(&resp).unwrap_or_default(),
                        );
                        Ok::<_, AppError>(serde_json::to_string_pretty(&resp).unwrap_or_default())
                    },
                    |r| Message::MessagesLoaded(r.map_err(|e| e.to_string())),
                )
            }
            Message::MessagesLoaded(Ok(json)) => {
                let (msgs, blink) = parse_messages(&json, &self.my_user_id);
                self.messages = msgs;
                self.backward_link = blink;
                self.messages_error = None;
                Task::none()
            }
            Message::MessagesLoaded(Err(e)) => {
                self.messages_error = Some(format!("Error: {e}"));
                Task::none()
            }
            Message::InputChanged(val) => {
                self.input_value = val;
                Task::none()
            }
            Message::SendPressed => {
                let content = self.input_value.trim().to_string();
                if content.is_empty() {
                    return Task::none();
                }
                self.input_value.clear();

                let c = self.client.clone();
                let chat_id = self.selected_chat_id.clone().unwrap_or_default();
                let name = self.my_display_name.clone();

                Task::perform(
                    async move {
                        let resp: serde_json::Value = c.send_message(&chat_id, &content, &name).await?;
                        Ok::<_, AppError>(serde_json::to_string_pretty(&resp).unwrap_or_default())
                    },
                    |r| Message::MessageSent(r.map_err(|e| e.to_string())),
                )
            }
            Message::MessageSent(Ok(_)) => {
                // Reload messages
                if let Some(id) = &self.selected_chat_id {
                    let c = self.client.clone();
                    let chat_id = id.clone();
                    return Task::perform(
                        async move {
                            let resp: serde_json::Value = c.list_messages(&chat_id).await?;
                            Ok::<_, AppError>(serde_json::to_string_pretty(&resp).unwrap_or_default())
                        },
                        |r| Message::MessagesLoaded(r.map_err(|e| e.to_string())),
                    );
                }
                Task::none()
            }
            Message::MessageSent(Err(e)) => {
                tracing::error!("Send failed: {e}");
                Task::none()
            }
            Message::ProfileLoaded(Ok(json)) => {
                // Update chat names from resolved user profiles
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&json) {
                    if let Some(users) = val.get("value").and_then(|v| v.as_array()) {
                        for user in users {
                            let mri = user.get("mri").and_then(|v| v.as_str()).unwrap_or("");
                            let display_name = user.get("displayName").and_then(|v| v.as_str()).unwrap_or("");
                            if mri.is_empty() || display_name.is_empty() { continue; }

                            // Extract bare UUID from MRI (8:orgid:uuid -> uuid)
                            let bare_uuid = mri.strip_prefix("8:orgid:").unwrap_or(mri);

                            // Find the matching chat and update its name
                            for chat in &mut self.chats {
                                if chat.id.contains(bare_uuid) && chat.display_name.starts_with("💬 User ") {
                                    tracing::info!("ProfileLoaded: {} -> 💬 {}", chat.display_name, display_name);
                                    chat.display_name = format!("💬 {display_name}");
                                }
                            }
                        }
                    }
                }

                // Log remaining unresolved chats
                let still_unresolved: Vec<&str> = self.chats.iter()
                    .filter(|c| c.display_name.starts_with("💬 User "))
                    .map(|c| c.id.as_str())
                    .collect();
                if !still_unresolved.is_empty() {
                    tracing::warn!(
                        "ProfileLoaded: {} chats still unresolved after users/fetch: {:?}",
                        still_unresolved.len(),
                        still_unresolved
                    );
                } else {
                    tracing::info!("ProfileLoaded: all chat names resolved!");
                }

                Task::none()
            }
            Message::ProfileLoaded(Err(_)) => Task::none(),
            Message::FederatedProfileLoaded(Ok(json)) => {
                // Update chat names from resolved federated user profiles
                let profiles = parse_federated_profiles(&json);
                tracing::info!("FederatedProfileLoaded: {} profiles resolved", profiles.len());

                for (mri, display_name) in &profiles {
                    let bare_uuid = mri.strip_prefix("8:orgid:").unwrap_or(mri);
                    for chat in &mut self.chats {
                        if chat.id.contains(bare_uuid) && chat.display_name.starts_with("💬 User ") {
                            tracing::info!("FederatedProfileLoaded: {} -> 💬 {}", chat.display_name, display_name);
                            chat.display_name = format!("💬 {display_name}");
                        }
                    }
                }

                // Log remaining unresolved chats
                let still_unresolved: Vec<&str> = self.chats.iter()
                    .filter(|c| c.display_name.starts_with("💬 User "))
                    .map(|c| c.id.as_str())
                    .collect();
                if !still_unresolved.is_empty() {
                    tracing::warn!(
                        "FederatedProfileLoaded: {} chats still unresolved: {:?}",
                        still_unresolved.len(),
                        still_unresolved
                    );
                } else {
                    tracing::info!("FederatedProfileLoaded: all chat names resolved!");
                }

                Task::none()
            }
            Message::FederatedProfileLoaded(Err(e)) => {
                tracing::warn!("fetchFederated failed: {e}");
                Task::none()
            }
            Message::OlderMessagesLoaded(Ok(json)) => {
                self.loading_older = false;
                let (older_msgs, blink) = parse_messages(&json, &self.my_user_id);
                self.backward_link = blink;
                // Prepend older messages (they come newest-first from API,
                // but we store them in API order and reverse in view)
                let mut combined = older_msgs;
                combined.extend(self.messages.drain(..));
                self.messages = combined;
                Task::none()
            }
            Message::OlderMessagesLoaded(Err(e)) => {
                self.loading_older = false;
                tracing::error!("Failed to load older messages: {e}");
                Task::none()
            }
            Message::UnreadCountLoaded(chat_id, count) => {
                if let Some(chat) = self.chats.iter_mut().find(|c| c.id == chat_id) {
                    let old = chat.unread_count;
                    // Only update if we got a real count; never reset to 0
                    // (initial detection already flagged the chat as unread)
                    if count > 0 {
                        chat.unread_count = count;
                        tracing::debug!("UnreadCountLoaded: {} -> {} (was {})", chat.display_name, count, old);
                    } else if old > 0 {
                        tracing::debug!("UnreadCountLoaded: {} returned 0, keeping old={}", chat.display_name, old);
                    }
                }
                Task::none()
            }
            Message::PollTick => {
                if let Some(id) = &self.selected_chat_id {
                    let c = self.client.clone();
                    let chat_id = id.clone();
                    return Task::perform(
                        async move {
                            let resp: serde_json::Value = c.list_messages(&chat_id).await?;
                            Ok::<_, AppError>(serde_json::to_string_pretty(&resp).unwrap_or_default())
                        },
                        |r| Message::PollMessagesLoaded(r.map_err(|e| e.to_string())),
                    );
                }
                Task::none()
            }
            Message::PollMessagesLoaded(Ok(json)) => {
                let (new_msgs, _) = parse_messages(&json, &self.my_user_id);
                // Merge: keep older messages, replace the newest page
                // Messages come newest-first from API. We need to find where old history ends
                // and append any new messages.
                if self.messages.len() <= new_msgs.len() {
                    // We only had the latest page anyway, just replace
                    self.messages = new_msgs;
                } else {
                    // Keep older messages (beyond the latest page), replace the tail
                    let older_count = self.messages.len() - new_msgs.len();
                    // But new messages might have more items if someone sent something
                    self.messages.truncate(older_count);
                    self.messages.extend(new_msgs);
                }
                Task::none()
            }
            Message::PollMessagesLoaded(Err(_)) => Task::none(),
            Message::ScrollChanged(viewport) => {
                // With anchor_bottom, absolute_offset_reversed().y == 0 means we're at the very top
                let dist_from_top = viewport.absolute_offset_reversed().y;
                // Auto-load when near top (200px threshold for no delay)
                if dist_from_top < 200.0
                    && !self.loading_older
                    && self.backward_link.is_some()
                {
                    // Auto-load older messages
                    self.loading_older = true;
                    let c = self.client.clone();
                    let url = self.backward_link.clone().unwrap();
                    Task::perform(
                        async move {
                            let resp: serde_json::Value = c.get_chatsvc_absolute(&url).await?;
                            Ok::<_, AppError>(serde_json::to_string_pretty(&resp).unwrap_or_default())
                        },
                        |r| Message::OlderMessagesLoaded(r.map_err(|e| e.to_string())),
                    )
                } else {
                    Task::none()
                }
            }
            Message::FoldersLoaded(Ok((folders, order))) => {
                tracing::info!("CSA folders loaded: {} folders, {} order entries", folders.len(), order.len());
                self.folders = folders;
                if !order.is_empty() {
                    self.folder_order = order;
                }
                Task::none()
            }
            Message::FoldersLoaded(Err(e)) => {
                tracing::error!("Failed to load folders from CSA: {e}");
                // Keep initial folders from IndexedDB
                Task::none()
            }
            Message::FolderToggle(folder_id) => {
                if self.collapsed_folders.contains(&folder_id) {
                    self.collapsed_folders.remove(&folder_id);
                } else {
                    self.collapsed_folders.insert(folder_id);
                }
                Task::none()
            }
            Message::RenameStart => {
                if let Some(name) = &self.selected_chat_name {
                    self.renaming_chat = true;
                    self.rename_value = name.clone();
                }
                Task::none()
            }
            Message::RenameChanged(val) => {
                self.rename_value = val;
                Task::none()
            }
            Message::RenameSubmit => {
                self.renaming_chat = false;
                let new_name = self.rename_value.trim().to_string();
                if let Some(chat_id) = &self.selected_chat_id {
                    if new_name.is_empty() {
                        // Remove custom name → revert to original
                        self.custom_chat_names.remove(chat_id);
                    } else {
                        self.custom_chat_names.insert(chat_id.clone(), new_name.clone());
                    }
                    save_custom_names(&self.custom_chat_names);

                    // Update selected_chat_name
                    if let Some(chat) = self.chats.iter().find(|c| c.id == *chat_id) {
                        let resolved = resolve_display_name(
                            chat_id,
                            &chat.display_name,
                            &self.custom_chat_names,
                        );
                        self.selected_chat_name = Some(resolved);
                    }
                }
                Task::none()
            }
        }
    }

    /// Render a single chat row for the sidebar (extracted for reuse in folder sections)
    fn render_chat_row<'a>(&'a self, chat: &'a ChatEntry) -> Element<'a, Message> {
        let is_selected = self.selected_chat_id.as_deref() == Some(chat.id.as_str());
        let props = chat_display_props(chat);

        // Unread indicator: colored vertical bar on the left
        let indicator: Element<'_, Message> = if props.show_indicator {
            container("")
                .width(3)
                .height(20)
                .style(move |_theme| container::Style {
                    background: Some(iced::Background::Color(props.indicator_color)),
                    ..Default::default()
                })
                .into()
        } else {
            container("")
                .width(3)
                .height(20)
                .into()
        };

        // Chat name — unread uses "Fira Sans Bold" font (loaded as separate family)
        // Do NOT use weight: Bold — iced 0.14 renders it invisible.
        // Instead, reference the bold TTF directly by its fullname as family.
        let name_font = if props.bold {
            iced::Font {
                family: iced::font::Family::Name("Fira Sans"),
                weight: iced::font::Weight::Bold,
                stretch: iced::font::Stretch::Normal,
                style: iced::font::Style::Normal,
            }
        } else {
            Font::with_name("Fira Sans")
        };
        let display = resolve_display_name(&chat.id, &chat.display_name, &self.custom_chat_names);
        let name_text = text(display.clone())
            .size(13)
            .color(props.text_color)
            .font(name_font);

        // Badge with unread count (if any)
        let content: Element<'_, Message> = if let Some(badge_str) = props.badge_text {
            let badge = container(
                text(badge_str).size(10).color(Color::WHITE)
            )
            .padding([1, 5])
            .style(|_theme| container::Style {
                background: Some(iced::Background::Color(Color::from_rgb(0.8, 0.2, 0.2))),
                border: iced::Border { color: Color::TRANSPARENT, width: 0.0, radius: 8.0.into() },
                ..Default::default()
            });
            row![
                name_text,
                iced::widget::Space::new().width(Length::Fill),
                badge,
            ]
            .width(Length::Fill)
            .align_y(iced::Alignment::Center)
            .into()
        } else {
            name_text.into()
        };

        let chat_row = row![indicator, content]
            .spacing(6)
            .width(Length::Fill)
            .align_y(iced::Alignment::Center);

        // Button: transparent bg, style depends on selected state
        let btn = button(chat_row)
            .on_press(Message::ChatSelected(chat.id.clone(), display))
            .width(Length::Fill)
            .padding([6, 8])
            .style(move |_theme, status| {
                let bg = match status {
                    _ if is_selected => TeamsDark::SELECTED,
                    iced::widget::button::Status::Hovered => TeamsDark::HOVER,
                    _ => Color::TRANSPARENT,
                };
                iced::widget::button::Style {
                    background: Some(iced::Background::Color(bg)),
                    text_color: Color::WHITE,
                    border: iced::Border { color: Color::TRANSPARENT, width: 0.0, radius: 4.0.into() },
                    ..Default::default()
                }
            });

        tid("C01", btn.into())
    }

    pub fn view(&self) -> Element<'_, Message> {
        // Build folder-grouped sidebar sections
        let sections = build_sidebar_sections(&self.chats, &self.folders, &self.folder_order);
        let has_real_folders = sections.iter().any(|s| s.folder_id != "___other___");

        let mut sidebar_items: Vec<Element<'_, Message>> = Vec::new();

        for section in sections {
            let is_other = section.folder_id == "___other___";
            let is_collapsed = self.collapsed_folders.contains(&section.folder_id);

            // Section header (skip "Sonstige" header if it's the only section)
            if is_other {
                if has_real_folders {
                    sidebar_items.push(
                        container(
                            text(format!("── {} ──", section.name))
                                .size(11)
                                .color(TeamsDark::TEXT_MUTED)
                                .font(Font::with_name("Fira Sans"))
                        )
                        .width(Length::Fill)
                        .padding([6, 8])
                        .into()
                    );
                }
            } else {
                // Folder header: chevron + name + chat count
                let chevron = if is_collapsed { "▸" } else { "▾" };
                let count = section.chat_indices.len();
                let folder_id = section.folder_id.clone();

                let header_content = row![
                    text(chevron).size(13).color(TeamsDark::TEXT_MUTED),
                    text(section.name.clone())
                        .size(13)
                        .color(TeamsDark::TEXT_MUTED)
                        .font(Font::with_name("Fira Sans")),
                    iced::widget::Space::new().width(Length::Fill),
                    text(format!("{count}"))
                        .size(10)
                        .color(TeamsDark::TEXT_MUTED),
                ]
                .spacing(4)
                .align_y(iced::Alignment::Center);

                let header_btn = button(header_content)
                    .on_press(Message::FolderToggle(folder_id))
                    .width(Length::Fill)
                    .padding([4, 8])
                    .style(|_theme, status| {
                        let bg = match status {
                            iced::widget::button::Status::Hovered => TeamsDark::HOVER,
                            _ => Color::TRANSPARENT,
                        };
                        iced::widget::button::Style {
                            background: Some(iced::Background::Color(bg)),
                            text_color: Color::WHITE,
                            border: iced::Border {
                                color: Color::TRANSPARENT,
                                width: 0.0,
                                radius: 4.0.into(),
                            },
                            ..Default::default()
                        }
                    });

                sidebar_items.push(header_btn.into());
            }

            // Chat items (skip if folder is collapsed and not "other")
            if !is_collapsed || is_other {
                for &idx in &section.chat_indices {
                    let chat = &self.chats[idx];
                    sidebar_items.push(self.render_chat_row(chat));
                }
            }
        }

        // Wrap each item with right margin so scrollbar doesn't cover content
        let sidebar_items_padded: Vec<Element<'_, Message>> = sidebar_items
            .into_iter()
            .map(|item| {
                row![item, iced::widget::Space::new().width(10)]
                    .width(Length::Fill)
                    .into()
            })
            .collect();

        let sidebar = tid("S01", container(
            scrollable(
                Column::with_children(sidebar_items_padded)
                    .spacing(2)
                    .width(Length::Fill)
            )
            .height(Length::Fill)
        )
        .width(290)
        .padding(8)
        .into());

        // Main area
        let main_area: Element<'_, Message> = if self.loading {
            center(text("Loading chats...").size(16)).into()
        } else if self.chats.is_empty() {
            // Show raw JSON for debugging
            center(
                scrollable(
                    text(&self.chats_json).size(11)
                ).height(Length::Fill)
            ).into()
        } else if let Some(chat_name) = &self.selected_chat_name {
            // Chat view with messages
            let header_style = |_theme: &iced::Theme| container::Style {
                background: Some(iced::Background::Color(TeamsDark::HEADER_BG)),
                border: iced::Border {
                    color: TeamsDark::BORDER,
                    width: 0.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            };

            let header: Element<'_, Message> = if self.renaming_chat {
                // Rename mode: text input
                tid("H01", container(
                    iced::widget::text_input("Chat name...", &self.rename_value)
                        .on_input(Message::RenameChanged)
                        .on_submit(Message::RenameSubmit)
                        .size(18)
                )
                .padding(12)
                .width(Length::Fill)
                .style(header_style)
                .into())
            } else {
                // Normal mode: clickable text
                tid("H01", container(
                    button(
                        text(chat_name).size(18).color(TeamsDark::TEXT_PRIMARY)
                    )
                    .on_press(Message::RenameStart)
                    .width(Length::Fill)
                    .style(|_theme, _status| iced::widget::button::Style {
                        background: None,
                        text_color: TeamsDark::TEXT_PRIMARY,
                        border: iced::Border::default(),
                        ..Default::default()
                    })
                )
                .padding(12)
                .width(Length::Fill)
                .style(header_style)
                .into())
            };

            // Build message bubbles
            let msg_view: Element<'_, Message> = if let Some(err) = &self.messages_error {
                center(text(err).size(14).color(Color::from_rgb(0.9, 0.3, 0.3))).into()
            } else if self.messages.is_empty() {
                center(text("Loading messages...").size(14).color(TeamsDark::TEXT_MUTED)).into()
            } else {
                // Messages come newest-first from API, reverse for chronological display
                let mut sorted_msgs: Vec<&ParsedMessage> = self.messages.iter().collect();
                sorted_msgs.reverse();

                // Check if there are messages from multiple senders
                // Only show names once both sides have sent something
                let has_multiple_senders = {
                    let mut seen_me = false;
                    let mut seen_other = false;
                    for m in &sorted_msgs {
                        if m.is_from_me { seen_me = true; } else { seen_other = true; }
                        if seen_me && seen_other { break; }
                    }
                    seen_me && seen_other
                };

                // Column widths: name = based on longest username, time = fixed
                let max_name_chars = sorted_msgs.iter()
                    .map(|m| m.sender_name.chars().count())
                    .max()
                    .unwrap_or(0);
                let name_col_w: f32 = (max_name_chars as f32) * 7.5 + 12.0;
                let time_col_w: f32 = 50.0; // fits "HH:MM"

                // Debug border style for layout visualization
                let debug_border = |_theme: &iced::Theme| container::Style {
                    border: iced::Border {
                        color: Color::from_rgb(0.4, 0.4, 0.4),
                        width: 1.0,
                        radius: 0.0.into(),
                    },
                    ..Default::default()
                };

                let mut msg_widgets: Vec<Element<'_, Message>> = Vec::new();

                // Loading indicator at top while fetching older messages
                if self.loading_older {
                    msg_widgets.push(
                        container(
                            text("Lade ältere Nachrichten...").size(11).color(TeamsDark::TEXT_MUTED)
                        )
                        .width(Length::Fill)
                        .align_x(iced::alignment::Horizontal::Center)
                        .padding([8, 0])
                        .into()
                    );
                }

                let mut last_sender: Option<&str> = None;
                let mut last_date: Option<&str> = None;
                let today = chrono::Utc::now().format("%Y-%m-%d").to_string();

                for msg in &sorted_msgs {
                    // Date separator: centered line when date changes (not today)
                    let msg_date = extract_date(&msg.timestamp);
                    let date_changed = last_date != Some(msg_date);
                    if date_changed && !msg_date.is_empty() && msg_date != today {
                        let label = format_date_separator(msg_date);
                        msg_widgets.push(
                            container(
                                text(label).size(11).color(TeamsDark::TEXT_MUTED)
                            )
                            .width(Length::Fill)
                            .align_x(iced::alignment::Horizontal::Center)
                            .padding([6, 0])
                            .into()
                        );
                        // Reset sender tracking after separator
                        last_sender = None;
                    }
                    last_date = Some(msg_date);

                    let sender_changed = last_sender != Some(&msg.sender_name);
                    last_sender = Some(&msg.sender_name);

                    let time_str = format_time(&msg.timestamp);
                    let show_name = has_multiple_senders && sender_changed;

                    let name_color = if msg.is_from_me {
                        Color::from_rgb(0.55, 0.65, 1.0)
                    } else {
                        Color::from_rgb(0.7, 0.85, 0.55)
                    };

                    // Col 1: Name
                    let name_text: Element<'_, Message> = if show_name {
                        text(&msg.sender_name).size(12).color(name_color).into()
                    } else {
                        text("").size(12).into()
                    };
                    let col1: Element<'_, Message> = container(name_text)
                        .width(name_col_w)
                        .padding([2, 4])
                        .style(debug_border)
                        .into();

                    // Col 2: Time
                    let col2: Element<'_, Message> = container(
                        text(time_str).size(11).color(TeamsDark::TEXT_MUTED)
                    )
                    .width(time_col_w)
                    .padding([2, 4])
                    .style(debug_border)
                    .into();

                    // Col 3: Message
                    let col3: Element<'_, Message> = container(
                        text(&msg.content).size(13).color(TeamsDark::TEXT_PRIMARY)
                    )
                    .padding([2, 4])
                    .style(debug_border)
                    .into();

                    let msg_row = iced::widget::Row::with_children(vec![col1, col2, col3])
                        .spacing(0);

                    msg_widgets.push(tid("M01", msg_row.into()));
                }

                scrollable(
                    Column::with_children(msg_widgets)
                        .spacing(2)
                        .padding(12)
                        .width(Length::Fill)
                )
                .on_scroll(Message::ScrollChanged)
                .height(Length::Fill)
                .anchor_bottom()
                .into()
            };

            let input: Element<'_, Message> = tid("I01", iced::widget::text_input("Type a message...", &self.input_value)
                .on_input(Message::InputChanged)
                .on_submit(Message::SendPressed)
                .width(Length::Fill)
                .into());

            let send: Element<'_, Message> = tid("B01", button(text("Send").size(14)).on_press(Message::SendPressed).into());
            let input_row = container(row![input, send].spacing(8)).padding(8);

            column![header, msg_view, input_row].into()
        } else {
            center(text("Select a chat").size(16)).into()
        };

        row![sidebar, main_area].into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        iced::time::every(std::time::Duration::from_secs(10))
            .map(|_| Message::PollTick)
    }
}

/// Try to parse chat conversations from the API response JSON
fn parse_chats(val: &serde_json::Value, my_user_id: &str) -> Vec<ChatEntry> {
    let mut chats = vec![];

    // Extract the bare user ID (without "8:orgid:" prefix) for comparison
    let my_bare_id = my_user_id
        .strip_prefix("8:orgid:")
        .unwrap_or(my_user_id);

    // Try "conversations" array (Teams Chat Service API format)
    if let Some(convs) = val.get("conversations").and_then(|v| v.as_array()) {
        for conv in convs {
            let id = conv.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if id.is_empty() { continue; }

            let thread_type = conv.get("threadProperties")
                .and_then(|tp| tp.get("threadType"))
                .and_then(|t| t.as_str())
                .unwrap_or("");

            // Skip calllogs, notifications, space (channels shown separately)
            if thread_type == "streamofcalllogs" || thread_type == "streamofnotifications" {
                continue;
            }

            // Detect unread: consumptionhorizon[0] < lastUpdatedMessageId
            let (is_unread, read_horizon_id) = {
                let last_msg_id = conv.get("lastUpdatedMessageId")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let consumption = conv.get("properties")
                    .and_then(|p| p.get("consumptionhorizon"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let read_id: u64 = consumption
                    .split(';')
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                (last_msg_id > read_id, read_id)
            };

            // Get display name
            let topic = conv.get("threadProperties")
                .and_then(|tp| tp.get("topic"))
                .and_then(|t| t.as_str())
                .unwrap_or("");

            let name = if !topic.is_empty() {
                topic.to_string()
            } else if let Some(members) = conv.get("members").and_then(|m| m.as_array()) {
                // Try getting names from members list
                let member_names: Vec<&str> = members.iter()
                    .filter(|m| {
                        let mid = m.get("id").and_then(|v| v.as_str()).unwrap_or("");
                        mid != my_user_id && !mid.contains(my_bare_id)
                    })
                    .filter_map(|m| m.get("friendlyName").and_then(|v| v.as_str()))
                    .filter(|n| !n.is_empty())
                    .collect();
                if !member_names.is_empty() {
                    member_names.join(", ")
                } else {
                    resolve_name_from_last_message(conv, my_bare_id)
                        .unwrap_or_else(|| extract_other_user_from_conv_id(&id, my_bare_id))
                }
            } else {
                // No members — try lastMessage.imdisplayname, then fallback to conv ID
                resolve_name_from_last_message(conv, my_bare_id)
                    .unwrap_or_else(|| {
                        if thread_type == "chat" {
                            extract_other_user_from_conv_id(&id, my_bare_id)
                        } else {
                            String::new()
                        }
                    })
            };

            // Add a type prefix for clarity
            let display_name = if name.is_empty() {
                id.chars().take(40).collect::<String>()
            } else {
                match thread_type {
                    "chat" => format!("💬 {name}"),
                    "topic" => format!("📋 {name}"),
                    "space" => format!("📌 {name}"),
                    "meeting" => format!("📅 {name}"),
                    _ => name,
                }
            };

            chats.push(ChatEntry {
                id,
                display_name,
                unread_count: if is_unread { 1 } else { 0 }, // placeholder, real count loaded async
                read_horizon_id,
            });
        }
    }

    // Try "value" array (Graph API format, fallback)
    if chats.is_empty() {
        if let Some(vals) = val.get("value").and_then(|v| v.as_array()) {
            for item in vals {
                let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let name = item.get("topic").and_then(|v| v.as_str()).unwrap_or(&id).to_string();
                if !id.is_empty() {
                    chats.push(ChatEntry {
                        id,
                        display_name: name,
                        unread_count: 0,
                        read_horizon_id: 0,
                    });
                }
            }
        }
    }

    chats
}

/// Merge chatsvc-sourced chat entries into the existing chat list.
///
/// - If a chat ID already exists: update its display name (unless incoming is a raw ID),
///   unread_count, and read_horizon_id
/// - If a chat ID is new: append it
///
/// Pure function, fully testable.
pub fn merge_chatsvc_into_existing(
    existing: &mut Vec<ChatEntry>,
    incoming: Vec<ChatEntry>,
) {
    // Build index of existing chats by ID for O(1) lookup
    let mut id_to_index: std::collections::HashMap<String, usize> = existing
        .iter()
        .enumerate()
        .map(|(i, c)| (c.id.clone(), i))
        .collect();

    for chat in incoming {
        if let Some(&idx) = id_to_index.get(&chat.id) {
            // Update existing entry
            let entry = &mut existing[idx];

            // Only overwrite display_name if incoming has a "real" name
            // (not a raw conversation ID like "19:xxx@thread.v2")
            let incoming_is_placeholder = chat.display_name.starts_with("19:")
                || chat.display_name.contains("@thread.v2")
                || chat.display_name.contains("@unq.gbl.spaces");
            if !incoming_is_placeholder {
                entry.display_name = chat.display_name;
            }

            entry.unread_count = chat.unread_count;
            entry.read_horizon_id = chat.read_horizon_id;
        } else {
            // New chat — append
            id_to_index.insert(chat.id.clone(), existing.len());
            existing.push(chat);
        }
    }
}

/// Try to resolve a chat name from `lastMessage.imdisplayname`.
/// Returns `Some(name)` if the last message was sent by someone else (not me).
/// Returns `None` if the last message is from me, empty, or missing.
fn resolve_name_from_last_message(
    conv: &serde_json::Value,
    my_bare_id: &str,
) -> Option<String> {
    let lm = conv.get("lastMessage")?;
    let from = lm.get("from").and_then(|v| v.as_str()).unwrap_or("");
    let name = lm.get("imdisplayname").and_then(|v| v.as_str()).unwrap_or("");

    if name.is_empty() {
        return None;
    }

    // If the last message is from me, the name is mine — not useful
    if from.contains(my_bare_id) {
        return None;
    }

    Some(name.to_string())
}

/// Extract the other user's ID from a 1:1 conversation ID
/// Format: 19:{user1-uuid}_{user2-uuid}@unq.gbl.spaces
fn extract_other_user_from_conv_id(conv_id: &str, my_bare_id: &str) -> String {
    // Strip "19:" prefix and "@..." suffix
    let inner = conv_id
        .strip_prefix("19:")
        .unwrap_or(conv_id)
        .split('@')
        .next()
        .unwrap_or("");

    // Split by "_" — should give two UUIDs
    let parts: Vec<&str> = inner.split('_').collect();
    if parts.len() == 2 {
        let other = if parts[0].starts_with(my_bare_id) || parts[0] == my_bare_id {
            parts[1]
        } else {
            parts[0]
        };
        // Return as short user reference
        if other.len() > 8 {
            format!("User {}", &other[..8])
        } else {
            format!("User {other}")
        }
    } else {
        inner.chars().take(30).collect()
    }
}

/// Parse messages from the Chat Service API JSON response
/// Returns (messages, backward_link) where backward_link is the URL for older messages
fn parse_messages(json: &str, my_user_id: &str) -> (Vec<ParsedMessage>, Option<String>) {
    let mut messages = vec![];

    let val: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return (messages, None),
    };

    // Extract backwardLink for pagination
    let backward_link = val
        .get("_metadata")
        .and_then(|m| m.get("backwardLink"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let my_bare_id = my_user_id
        .strip_prefix("8:orgid:")
        .unwrap_or(my_user_id);

    if let Some(msgs) = val.get("messages").and_then(|v| v.as_array()) {
        for msg in msgs {
            // Only render actual messages
            let msg_type = msg.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if msg_type != "Message" {
                continue;
            }

            let sender_name = msg.get("imdisplayname")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown")
                .to_string();

            // Extract sender ID from "from" URL
            // Format: .../contacts/8:orgid:uuid
            let from_url = msg.get("from").and_then(|v| v.as_str()).unwrap_or("");
            let sender_id = from_url
                .rsplit('/')
                .next()
                .unwrap_or("")
                .to_string();

            let is_from_me = sender_id.contains(my_bare_id);

            let raw_content = msg.get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            // Strip HTML tags and decode common entities
            let content = strip_html(raw_content)
                .replace("&nbsp;", " ")
                .replace("&amp;", "&")
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&quot;", "\"");

            // Skip empty messages (system events etc.)
            if content.trim().is_empty() {
                continue;
            }

            let timestamp = msg.get("composetime")
                .or_else(|| msg.get("originalarrivaltime"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            messages.push(ParsedMessage {
                sender_name,
                sender_id,
                content,
                timestamp,
                is_from_me,
            });
        }
    }

    (messages, backward_link)
}

/// Format ISO 8601 timestamp → "HH:MM" only
fn format_time(iso: &str) -> String {
    let parts: Vec<&str> = iso.split('T').collect();
    if parts.len() < 2 {
        return iso.chars().take(5).collect();
    }
    let time_part = parts[1];
    let hm: String = time_part.chars().take(5).collect();
    if hm.len() == 5 { hm } else { iso.chars().take(5).collect() }
}

/// Extract date part "YYYY-MM-DD" from ISO timestamp
fn extract_date(iso: &str) -> &str {
    iso.split('T').next().unwrap_or("")
}

/// Format "YYYY-MM-DD" → "Montag, 23.03.2026"
fn format_date_separator(ymd: &str) -> String {
    let fields: Vec<&str> = ymd.split('-').collect();
    if fields.len() != 3 {
        return ymd.to_string();
    }
    let year: i32 = fields[0].parse().unwrap_or(2026);
    let month: u32 = fields[1].parse().unwrap_or(1);
    let day: u32 = fields[2].parse().unwrap_or(1);

    if let Some(date) = chrono::NaiveDate::from_ymd_opt(year, month, day) {
        let weekday = match date.format("%A").to_string().as_str() {
            "Monday" => "Montag",
            "Tuesday" => "Dienstag",
            "Wednesday" => "Mittwoch",
            "Thursday" => "Donnerstag",
            "Friday" => "Freitag",
            "Saturday" => "Samstag",
            "Sunday" => "Sonntag",
            other => return format!("{}, {:02}.{:02}.{}", other, day, month, year),
        };
        format!("{}, {:02}.{:02}.{}", weekday, day, month, year)
    } else {
        ymd.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_chat(unread_count: u32) -> ChatEntry {
        ChatEntry {
            id: "19:test@unq.gbl.spaces".into(),
            display_name: "💬 Test User".into(),
            unread_count,
            read_horizon_id: 100,
        }
    }

    // ── ChatDisplayProps contract ──────────────────────────────────

    // ── unread display tests ──

    #[test]
    fn unread_chat_is_bold() {
        let props = chat_display_props(&make_chat(3));
        assert!(props.bold, "unread chat name must be bold");
    }

    #[test]
    fn read_chat_is_not_bold() {
        let props = chat_display_props(&make_chat(0));
        assert!(!props.bold, "read chat name must not be bold");
    }

    #[test]
    fn unread_chat_has_white_text() {
        let props = chat_display_props(&make_chat(3));
        assert_eq!(props.text_color, Color::WHITE);
    }

    #[test]
    fn read_chat_has_white_text() {
        let props = chat_display_props(&make_chat(0));
        assert_eq!(props.text_color, Color::WHITE);
    }

    #[test]
    fn unread_chat_shows_indicator() {
        let props = chat_display_props(&make_chat(5));
        assert!(props.show_indicator, "unread chat must show indicator bar");
    }

    #[test]
    fn read_chat_hides_indicator() {
        let props = chat_display_props(&make_chat(0));
        assert!(!props.show_indicator, "read chat must not show indicator bar");
    }

    #[test]
    fn unread_chat_shows_badge_with_count() {
        let props = chat_display_props(&make_chat(7));
        assert_eq!(props.badge_text, Some("7".to_string()));
    }

    #[test]
    fn unread_chat_10_plus_shows_9_plus() {
        let props = chat_display_props(&make_chat(10));
        assert_eq!(props.badge_text, Some("9+".to_string()));

        let props = chat_display_props(&make_chat(42));
        assert_eq!(props.badge_text, Some("9+".to_string()));
    }

    #[test]
    fn read_chat_has_no_badge() {
        let props = chat_display_props(&make_chat(0));
        assert_eq!(props.badge_text, None);
    }

    #[test]
    fn unread_1_shows_badge_1() {
        let props = chat_display_props(&make_chat(1));
        assert_eq!(props.badge_text, Some("1".to_string()));
        assert!(props.show_indicator);
    }

    // ── parse_chats ───────────────────────────────────────────────

    #[test]
    fn parse_chats_detects_unread() {
        let json = serde_json::json!({
            "conversations": [{
                "id": "19:abc@unq.gbl.spaces",
                "lastUpdatedMessageId": 200,
                "threadProperties": { "threadType": "chat", "topic": "" },
                "properties": { "consumptionhorizon": "100;100;100" },
                "members": []
            }, {
                "id": "19:def@unq.gbl.spaces",
                "lastUpdatedMessageId": 100,
                "threadProperties": { "threadType": "chat", "topic": "" },
                "properties": { "consumptionhorizon": "100;100;100" },
                "members": []
            }]
        });

        let chats = parse_chats(&json, "8:orgid:abc");
        assert_eq!(chats.len(), 2);
        assert!(chats[0].unread_count > 0, "chat with newer messages should be unread");
        assert_eq!(chats[0].read_horizon_id, 100);
        assert_eq!(chats[1].unread_count, 0, "chat with no new messages should be read");
    }

    // ── build_sidebar_sections ────────────────────────────────────

    fn make_folder(id: &str, name: &str, conv_ids: Vec<&str>) -> CsaFolder {
        CsaFolder {
            id: id.to_string(),
            name: name.to_string(),
            folder_type: "UserCreated".to_string(),
            sort_type: "UserDefinedCustomOrder".to_string(),
            is_expanded: true,
            is_deleted: false,
            version: 1,
            conversations: conv_ids
                .into_iter()
                .map(|cid| CsaFolderConversation {
                    id: cid.to_string(),
                    thread_type: "chat".to_string(),
                })
                .collect(),
        }
    }

    fn make_chat_entry(id: &str, name: &str) -> ChatEntry {
        ChatEntry {
            id: id.to_string(),
            display_name: name.to_string(),
            unread_count: 0,
            read_horizon_id: 0,
        }
    }

    #[test]
    fn sidebar_sections_groups_chats_by_folder() {
        let chats = vec![
            make_chat_entry("19:aaa@thread.v2", "Chat A"),
            make_chat_entry("19:bbb@thread.v2", "Chat B"),
            make_chat_entry("19:ccc@thread.v2", "Chat C"),
            make_chat_entry("19:ddd@thread.v2", "Chat D"),
        ];
        let folders = vec![
            make_folder("folder-1", "Engineering", vec!["19:aaa@thread.v2", "19:ccc@thread.v2"]),
            make_folder("folder-2", "Design", vec!["19:bbb@thread.v2", "19:ddd@thread.v2"]),
        ];
        let order = vec!["folder-1".to_string(), "folder-2".to_string()];

        let sections = build_sidebar_sections(&chats, &folders, &order);

        // Should have 2 folder sections (no "Sonstige" because all chats matched)
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].name, "Engineering");
        assert_eq!(sections[0].chat_indices, vec![0, 2]);
        assert_eq!(sections[1].name, "Design");
        assert_eq!(sections[1].chat_indices, vec![1, 3]);
    }

    #[test]
    fn sidebar_sections_puts_unmatched_in_other() {
        let chats = vec![
            make_chat_entry("19:aaa@thread.v2", "Chat A"),
            make_chat_entry("19:zzz@thread.v2", "Orphan Chat"),
        ];
        let folders = vec![
            make_folder("folder-1", "Work", vec!["19:aaa@thread.v2"]),
        ];
        let order = vec!["folder-1".to_string()];

        let sections = build_sidebar_sections(&chats, &folders, &order);

        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].name, "Work");
        assert_eq!(sections[0].chat_indices, vec![0]);
        assert_eq!(sections[1].folder_id, "___other___");
        assert_eq!(sections[1].name, "Sonstige");
        assert_eq!(sections[1].chat_indices, vec![1]);
    }

    #[test]
    fn sidebar_sections_respects_folder_order() {
        let chats = vec![
            make_chat_entry("19:aaa@thread.v2", "Chat A"),
        ];
        let folders = vec![
            make_folder("folder-b", "Beta", vec![]),
            make_folder("folder-a", "Alpha", vec!["19:aaa@thread.v2"]),
        ];
        // Order puts Beta before Alpha
        let order = vec!["folder-b".to_string(), "folder-a".to_string()];

        let sections = build_sidebar_sections(&chats, &folders, &order);

        // folder-b first (even though empty), then folder-a, then Sonstige only if needed
        assert!(sections.len() >= 2);
        assert_eq!(sections[0].name, "Beta");
        assert_eq!(sections[1].name, "Alpha");
        assert_eq!(sections[1].chat_indices, vec![0]);
    }

    #[test]
    fn sidebar_sections_skips_deleted_folders() {
        let chats = vec![
            make_chat_entry("19:aaa@thread.v2", "Chat A"),
        ];
        let mut deleted_folder = make_folder("folder-del", "Deleted", vec!["19:aaa@thread.v2"]);
        deleted_folder.is_deleted = true;
        let folders = vec![deleted_folder];
        let order = vec!["folder-del".to_string()];

        let sections = build_sidebar_sections(&chats, &folders, &order);

        // Deleted folder skipped, chat goes to "Sonstige"
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].folder_id, "___other___");
        assert_eq!(sections[0].chat_indices, vec![0]);
    }

    #[test]
    fn sidebar_sections_empty_folder_still_shown() {
        let chats = vec![
            make_chat_entry("19:aaa@thread.v2", "Chat A"),
        ];
        let folders = vec![
            make_folder("folder-empty", "Empty Folder", vec![]),
            make_folder("folder-full", "Has Chat", vec!["19:aaa@thread.v2"]),
        ];
        let order = vec!["folder-empty".to_string(), "folder-full".to_string()];

        let sections = build_sidebar_sections(&chats, &folders, &order);

        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].name, "Empty Folder");
        assert!(sections[0].chat_indices.is_empty());
        assert_eq!(sections[1].name, "Has Chat");
        assert_eq!(sections[1].chat_indices, vec![0]);
    }

    #[test]
    fn sidebar_sections_no_folders_returns_single_section() {
        let chats = vec![
            make_chat_entry("19:aaa@thread.v2", "Chat A"),
            make_chat_entry("19:bbb@thread.v2", "Chat B"),
        ];
        let folders: Vec<CsaFolder> = vec![];
        let order: Vec<String> = vec![];

        let sections = build_sidebar_sections(&chats, &folders, &order);

        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].folder_id, "___other___");
        assert_eq!(sections[0].chat_indices, vec![0, 1]);
    }

    // ── build_sidebar_sections: edge cases & robustness ───────────

    #[test]
    fn sidebar_sections_chat_in_multiple_folders_appears_in_all() {
        // Same chat referenced by two folders — should appear in both
        let chats = vec![
            make_chat_entry("19:aaa@thread.v2", "Chat A"),
        ];
        let folders = vec![
            make_folder("folder-1", "First", vec!["19:aaa@thread.v2"]),
            make_folder("folder-2", "Second", vec!["19:aaa@thread.v2"]),
        ];
        let order = vec!["folder-1".to_string(), "folder-2".to_string()];

        let sections = build_sidebar_sections(&chats, &folders, &order);

        // Chat appears in both folders
        assert_eq!(sections[0].name, "First");
        assert_eq!(sections[0].chat_indices, vec![0]);
        assert_eq!(sections[1].name, "Second");
        assert_eq!(sections[1].chat_indices, vec![0]);
        // No "Sonstige" because all chats are assigned to at least one folder
        assert_eq!(sections.len(), 2);
    }

    #[test]
    fn sidebar_sections_order_references_nonexistent_folder() {
        // folder_order contains an ID that doesn't exist in folders
        let chats = vec![
            make_chat_entry("19:aaa@thread.v2", "Chat A"),
        ];
        let folders = vec![
            make_folder("folder-real", "Real", vec!["19:aaa@thread.v2"]),
        ];
        let order = vec![
            "folder-ghost".to_string(),
            "folder-real".to_string(),
        ];

        let sections = build_sidebar_sections(&chats, &folders, &order);

        // Ghost folder skipped, only "Real" section present
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].name, "Real");
        assert_eq!(sections[0].chat_indices, vec![0]);
    }

    #[test]
    fn sidebar_sections_folder_not_in_order_is_ignored() {
        // Folder exists but is not in folder_order — should not appear
        let chats = vec![
            make_chat_entry("19:aaa@thread.v2", "Chat A"),
        ];
        let folders = vec![
            make_folder("folder-orphan", "Orphan Folder", vec!["19:aaa@thread.v2"]),
        ];
        let order: Vec<String> = vec![]; // empty order

        let sections = build_sidebar_sections(&chats, &folders, &order);

        // No folder sections, everything in "Sonstige"
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].folder_id, "___other___");
        assert_eq!(sections[0].chat_indices, vec![0]);
    }

    #[test]
    fn sidebar_sections_empty_chats_and_empty_folders() {
        let chats: Vec<ChatEntry> = vec![];
        let folders: Vec<CsaFolder> = vec![];
        let order: Vec<String> = vec![];

        let sections = build_sidebar_sections(&chats, &folders, &order);

        // Single "Sonstige" section with no chats
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].folder_id, "___other___");
        assert!(sections[0].chat_indices.is_empty());
    }

    #[test]
    fn sidebar_sections_favorites_is_system() {
        let chats = vec![
            make_chat_entry("19:aaa@thread.v2", "Chat A"),
        ];
        let mut fav = make_folder("fav-id", "Favorites", vec!["19:aaa@thread.v2"]);
        fav.folder_type = "Favorites".to_string();
        let folders = vec![fav];
        let order = vec!["fav-id".to_string()];

        let sections = build_sidebar_sections(&chats, &folders, &order);

        assert_eq!(sections[0].name, "Favorites");
        assert!(sections[0].is_system, "Favorites folder must be marked as system");
    }

    #[test]
    fn sidebar_sections_quickviews_is_system() {
        let chats: Vec<ChatEntry> = vec![];
        let mut qv = make_folder("qv-id", "QuickViews", vec![]);
        qv.folder_type = "QuickViews".to_string();
        let folders = vec![qv];
        let order = vec!["qv-id".to_string()];

        let sections = build_sidebar_sections(&chats, &folders, &order);

        assert!(sections[0].is_system, "QuickViews folder must be marked as system");
    }

    #[test]
    fn sidebar_sections_user_created_is_not_system() {
        let chats: Vec<ChatEntry> = vec![];
        let folders = vec![
            make_folder("uc-id", "My Folder", vec![]),
        ];
        let order = vec!["uc-id".to_string()];

        let sections = build_sidebar_sections(&chats, &folders, &order);

        assert!(!sections[0].is_system, "UserCreated folder must not be marked as system");
    }

    #[test]
    fn sidebar_sections_preserves_chat_index_mapping() {
        // Verify indices correctly map back to the original chats array
        let chats = vec![
            make_chat_entry("19:aaa@thread.v2", "Alpha"),   // idx 0
            make_chat_entry("19:bbb@thread.v2", "Bravo"),   // idx 1
            make_chat_entry("19:ccc@thread.v2", "Charlie"), // idx 2
            make_chat_entry("19:ddd@thread.v2", "Delta"),   // idx 3
            make_chat_entry("19:eee@thread.v2", "Echo"),    // idx 4
        ];
        let folders = vec![
            make_folder("f1", "Group1", vec!["19:bbb@thread.v2", "19:ddd@thread.v2"]),
            make_folder("f2", "Group2", vec!["19:aaa@thread.v2"]),
        ];
        let order = vec!["f1".to_string(), "f2".to_string()];

        let sections = build_sidebar_sections(&chats, &folders, &order);

        // Group1 has Bravo(1) and Delta(3)
        assert_eq!(sections[0].chat_indices, vec![1, 3]);
        // Group2 has Alpha(0)
        assert_eq!(sections[1].chat_indices, vec![0]);
        // Sonstige has Charlie(2) and Echo(4)
        assert_eq!(sections[2].folder_id, "___other___");
        assert_eq!(sections[2].chat_indices, vec![2, 4]);

        // Verify round-trip: indices actually point to correct chats
        assert_eq!(chats[sections[0].chat_indices[0]].display_name, "Bravo");
        assert_eq!(chats[sections[0].chat_indices[1]].display_name, "Delta");
        assert_eq!(chats[sections[1].chat_indices[0]].display_name, "Alpha");
        assert_eq!(chats[sections[2].chat_indices[0]].display_name, "Charlie");
        assert_eq!(chats[sections[2].chat_indices[1]].display_name, "Echo");
    }

    #[test]
    fn sidebar_sections_many_deleted_folders_all_skipped() {
        let chats = vec![
            make_chat_entry("19:aaa@thread.v2", "Chat A"),
            make_chat_entry("19:bbb@thread.v2", "Chat B"),
        ];
        let mut d1 = make_folder("d1", "Deleted1", vec!["19:aaa@thread.v2"]);
        d1.is_deleted = true;
        let mut d2 = make_folder("d2", "Deleted2", vec!["19:bbb@thread.v2"]);
        d2.is_deleted = true;
        let mut d3 = make_folder("d3", "Deleted3", vec![]);
        d3.is_deleted = true;
        let folders = vec![d1, d2, d3];
        let order = vec!["d1".to_string(), "d2".to_string(), "d3".to_string()];

        let sections = build_sidebar_sections(&chats, &folders, &order);

        // All deleted — only "Sonstige" with both chats
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].folder_id, "___other___");
        assert_eq!(sections[0].chat_indices, vec![0, 1]);
    }

    #[test]
    fn sidebar_sections_no_sonstige_when_all_chats_assigned() {
        // If every chat is in a folder, no "Sonstige" section should appear
        let chats = vec![
            make_chat_entry("19:aaa@thread.v2", "Chat A"),
            make_chat_entry("19:bbb@thread.v2", "Chat B"),
        ];
        let folders = vec![
            make_folder("f1", "All Chats", vec!["19:aaa@thread.v2", "19:bbb@thread.v2"]),
        ];
        let order = vec!["f1".to_string()];

        let sections = build_sidebar_sections(&chats, &folders, &order);

        assert_eq!(sections.len(), 1, "no Sonstige section when all chats are assigned");
        assert_eq!(sections[0].name, "All Chats");
        assert_eq!(sections[0].chat_indices, vec![0, 1]);
    }

    // ── parse_csa_chats ───────────────────────────────────────────

    fn make_csa_chat(id: &str, title: Option<&str>, thread_type: &str) -> CsaChat {
        CsaChat {
            id: id.to_string(),
            title: title.map(|s| s.to_string()),
            thread_type: thread_type.to_string(),
            is_read: true,
            is_one_on_one: false,
            hidden: false,
            is_disabled: false,
            is_conversation_deleted: false,
            members: vec![],
            last_message: None,
            consumption_horizon: None,
        }
    }

    #[test]
    fn csa_chat_with_title_uses_title() {
        let chats = vec![make_csa_chat("19:aaa@thread.v2", Some("Project Alpha"), "chat")];
        let result = parse_csa_chats(&chats, "8:orgid:me");

        assert_eq!(result.len(), 1);
        assert!(result[0].display_name.contains("Project Alpha"));
    }

    #[test]
    fn csa_chat_type_prefix_chat() {
        let chats = vec![make_csa_chat("19:aaa@thread.v2", Some("Test"), "chat")];
        let result = parse_csa_chats(&chats, "8:orgid:me");
        assert!(result[0].display_name.starts_with("💬"), "chat should have 💬 prefix");
    }

    #[test]
    fn csa_chat_type_prefix_meeting() {
        let chats = vec![make_csa_chat("19:aaa@thread.v2", Some("Standup"), "meeting")];
        let result = parse_csa_chats(&chats, "8:orgid:me");
        assert!(result[0].display_name.starts_with("📅"), "meeting should have 📅 prefix");
    }

    #[test]
    fn csa_hidden_chat_is_skipped() {
        let mut chat = make_csa_chat("19:aaa@thread.v2", Some("Hidden"), "chat");
        chat.hidden = true;
        let result = parse_csa_chats(&[chat], "8:orgid:me");
        assert!(result.is_empty(), "hidden chats must be skipped");
    }

    #[test]
    fn csa_deleted_chat_is_skipped() {
        let mut chat = make_csa_chat("19:aaa@thread.v2", Some("Deleted"), "chat");
        chat.is_conversation_deleted = true;
        let result = parse_csa_chats(&[chat], "8:orgid:me");
        assert!(result.is_empty(), "deleted chats must be skipped");
    }

    #[test]
    fn csa_unread_chat_has_unread_count() {
        let mut chat = make_csa_chat("19:aaa@thread.v2", Some("Unread"), "chat");
        chat.is_read = false;
        let result = parse_csa_chats(&[chat], "8:orgid:me");
        assert!(result[0].unread_count > 0, "unread chat must have unread_count > 0");
    }

    #[test]
    fn csa_read_chat_has_zero_unread() {
        let chat = make_csa_chat("19:aaa@thread.v2", Some("Read"), "chat");
        let result = parse_csa_chats(&[chat], "8:orgid:me");
        assert_eq!(result[0].unread_count, 0, "read chat must have unread_count = 0");
    }

    #[test]
    fn csa_one_on_one_without_title_uses_member_name() {
        let mut chat = make_csa_chat("19:abc_def@unq.gbl.spaces", None, "chat");
        chat.is_one_on_one = true;
        chat.members = vec![
            CsaMember { mri: "8:orgid:me-id".into(), object_id: "me-id".into(), role: "Admin".into(), is_muted: false },
            CsaMember { mri: "8:orgid:other-id".into(), object_id: "other-id".into(), role: "User".into(), is_muted: false },
        ];
        chat.last_message = Some(CsaLastMessage {
            id: "123".into(),
            im_display_name: "Feenstra, Vinzenz".into(),
            content: "Hi".into(),
            compose_time: "2026-04-07T19:00:00Z".into(),
            message_type: "Text".into(),
            from: "8:orgid:other-id".into(),
        });
        let result = parse_csa_chats(&[chat], "8:orgid:me-id");

        assert_eq!(result.len(), 1);
        // Should use last_message.imDisplayName or member info, not show empty
        assert!(!result[0].display_name.is_empty());
        assert!(!result[0].display_name.contains("User "), "should have resolved name, not 'User ...'");
    }

    #[test]
    fn csa_empty_input_returns_empty() {
        let result = parse_csa_chats(&[], "8:orgid:me");
        assert!(result.is_empty());
    }

    #[test]
    fn csa_disabled_chat_is_skipped() {
        let mut chat = make_csa_chat("19:aaa@thread.v2", Some("Disabled"), "chat");
        chat.is_disabled = true;
        let result = parse_csa_chats(&[chat], "8:orgid:me");
        assert!(result.is_empty(), "disabled chats must be skipped");
    }

    // ── fill_missing_folder_chats ─────────────────────────────────

    #[test]
    fn fill_missing_adds_placeholder_for_unknown_chat() {
        let mut chats = vec![
            make_chat_entry("19:aaa@thread.v2", "Chat A"),
        ];
        let folders = vec![
            make_folder("f1", "Group", vec!["19:aaa@thread.v2", "19:bbb@thread.v2"]),
        ];

        fill_missing_folder_chats(&mut chats, &folders);

        assert_eq!(chats.len(), 2);
        assert_eq!(chats[1].id, "19:bbb@thread.v2");
    }

    #[test]
    fn fill_missing_does_not_duplicate_existing() {
        let mut chats = vec![
            make_chat_entry("19:aaa@thread.v2", "Chat A"),
        ];
        let folders = vec![
            make_folder("f1", "Group", vec!["19:aaa@thread.v2"]),
        ];

        fill_missing_folder_chats(&mut chats, &folders);

        assert_eq!(chats.len(), 1, "must not duplicate existing chat");
    }

    #[test]
    fn fill_missing_skips_deleted_folders() {
        let mut chats: Vec<ChatEntry> = vec![];
        let mut folder = make_folder("f1", "Deleted", vec!["19:aaa@thread.v2"]);
        folder.is_deleted = true;

        fill_missing_folder_chats(&mut chats, &[folder]);

        assert!(chats.is_empty(), "deleted folder conversations must not be added");
    }

    #[test]
    fn fill_missing_placeholder_has_thread_type_prefix() {
        let mut chats: Vec<ChatEntry> = vec![];
        let mut folder = make_folder("f1", "Mixed", vec!["19:aaa@thread.v2"]);
        folder.conversations[0].thread_type = "meeting".to_string();

        fill_missing_folder_chats(&mut chats, &[folder]);

        assert!(chats[0].display_name.starts_with("📅"), "meeting placeholder must have 📅 prefix");
    }

    #[test]
    fn fill_missing_multiple_folders() {
        let mut chats = vec![
            make_chat_entry("19:aaa@thread.v2", "Chat A"),
        ];
        let folders = vec![
            make_folder("f1", "Group1", vec!["19:bbb@thread.v2"]),
            make_folder("f2", "Group2", vec!["19:ccc@thread.v2"]),
        ];

        fill_missing_folder_chats(&mut chats, &folders);

        assert_eq!(chats.len(), 3);
        let ids: Vec<&str> = chats.iter().map(|c| c.id.as_str()).collect();
        assert!(ids.contains(&"19:bbb@thread.v2"));
        assert!(ids.contains(&"19:ccc@thread.v2"));
    }

    // ── merge_chatsvc_into_existing ─────────────────────────────────

    #[test]
    fn merge_updates_placeholder_with_real_name() {
        let mut existing = vec![
            make_chat_entry("19:aaa@thread.v2", "💬 aaa...v2"),
        ];
        let incoming = vec![
            ChatEntry {
                id: "19:aaa@thread.v2".into(),
                display_name: "💬 Alice".into(),
                unread_count: 0,
                read_horizon_id: 500,
            },
        ];

        merge_chatsvc_into_existing(&mut existing, incoming);

        assert_eq!(existing.len(), 1);
        assert_eq!(existing[0].display_name, "💬 Alice");
        assert_eq!(existing[0].read_horizon_id, 500);
    }

    #[test]
    fn merge_adds_new_chats() {
        let mut existing = vec![
            make_chat_entry("19:aaa@thread.v2", "💬 Alice"),
        ];
        let incoming = vec![
            ChatEntry {
                id: "19:bbb@thread.v2".into(),
                display_name: "💬 Bob".into(),
                unread_count: 1,
                read_horizon_id: 200,
            },
        ];

        merge_chatsvc_into_existing(&mut existing, incoming);

        assert_eq!(existing.len(), 2);
        assert_eq!(existing[1].id, "19:bbb@thread.v2");
        assert_eq!(existing[1].display_name, "💬 Bob");
    }

    #[test]
    fn merge_preserves_unmodified_chats() {
        let mut existing = vec![
            make_chat_entry("19:aaa@thread.v2", "💬 Alice"),
            make_chat_entry("19:bbb@thread.v2", "💬 Bob"),
        ];
        let incoming = vec![
            ChatEntry {
                id: "19:aaa@thread.v2".into(),
                display_name: "💬 Alice Updated".into(),
                unread_count: 0,
                read_horizon_id: 600,
            },
        ];

        merge_chatsvc_into_existing(&mut existing, incoming);

        assert_eq!(existing.len(), 2);
        assert_eq!(existing[0].display_name, "💬 Alice Updated");
        assert_eq!(existing[1].display_name, "💬 Bob"); // unchanged
    }

    #[test]
    fn merge_updates_unread_count() {
        let mut existing = vec![
            ChatEntry {
                id: "19:aaa@thread.v2".into(),
                display_name: "💬 Alice".into(),
                unread_count: 0,
                read_horizon_id: 100,
            },
        ];
        let incoming = vec![
            ChatEntry {
                id: "19:aaa@thread.v2".into(),
                display_name: "💬 Alice".into(),
                unread_count: 3,
                read_horizon_id: 100,
            },
        ];

        merge_chatsvc_into_existing(&mut existing, incoming);

        assert_eq!(existing[0].unread_count, 3);
    }

    #[test]
    fn merge_does_not_overwrite_real_name_with_placeholder() {
        let mut existing = vec![
            ChatEntry {
                id: "19:aaa@thread.v2".into(),
                display_name: "💬 Alice".into(),
                unread_count: 1,
                read_horizon_id: 100,
            },
        ];
        // Incoming has an empty/truncated name — should NOT overwrite a good name
        let incoming = vec![
            ChatEntry {
                id: "19:aaa@thread.v2".into(),
                display_name: "19:aaa@thread.v2".into(),
                unread_count: 0,
                read_horizon_id: 200,
            },
        ];

        merge_chatsvc_into_existing(&mut existing, incoming);

        // Name should stay as "Alice" since incoming is just the raw ID
        assert_eq!(existing[0].display_name, "💬 Alice");
        // But read_horizon should be updated
        assert_eq!(existing[0].read_horizon_id, 200);
    }

    #[test]
    fn merge_empty_incoming_is_noop() {
        let mut existing = vec![
            make_chat_entry("19:aaa@thread.v2", "💬 Alice"),
        ];
        let incoming = vec![];

        merge_chatsvc_into_existing(&mut existing, incoming);

        assert_eq!(existing.len(), 1);
        assert_eq!(existing[0].display_name, "💬 Alice");
    }

    #[test]
    fn merge_into_empty_existing() {
        let mut existing: Vec<ChatEntry> = vec![];
        let incoming = vec![
            ChatEntry {
                id: "19:aaa@thread.v2".into(),
                display_name: "💬 Alice".into(),
                unread_count: 0,
                read_horizon_id: 100,
            },
        ];

        merge_chatsvc_into_existing(&mut existing, incoming);

        assert_eq!(existing.len(), 1);
        assert_eq!(existing[0].display_name, "💬 Alice");
    }

    // ── resolve_display_name ────────────────────────────────────────

    #[test]
    fn resolve_custom_name_has_priority() {
        let mut custom = HashMap::new();
        custom.insert("19:abc@thread.v2".to_string(), "Mein Projekt".to_string());

        let result = resolve_display_name("19:abc@thread.v2", "💬 Original", &custom);
        assert_eq!(result, "Mein Projekt");
    }

    #[test]
    fn resolve_no_custom_returns_original() {
        let custom = HashMap::new();

        let result = resolve_display_name("19:abc@thread.v2", "💬 Original", &custom);
        assert_eq!(result, "💬 Original");
    }

    #[test]
    fn resolve_empty_custom_returns_original() {
        let mut custom = HashMap::new();
        custom.insert("19:abc@thread.v2".to_string(), "".to_string());

        let result = resolve_display_name("19:abc@thread.v2", "💬 Original", &custom);
        assert_eq!(result, "💬 Original");
    }

    #[test]
    fn resolve_different_id_returns_original() {
        let mut custom = HashMap::new();
        custom.insert("19:other@thread.v2".to_string(), "Custom".to_string());

        let result = resolve_display_name("19:abc@thread.v2", "💬 Original", &custom);
        assert_eq!(result, "💬 Original");
    }

    // ── resolve_name_from_last_message ──────────────────────────────

    #[test]
    fn last_message_from_other_user_resolves_name() {
        let conv = serde_json::json!({
            "lastMessage": {
                "imdisplayname": "Müller, Klaus",
                "from": "https://teams.microsoft.com/api/chatsvc/de/v1/users/ME/contacts/8:orgid:other-uuid"
            }
        });
        assert_eq!(
            resolve_name_from_last_message(&conv, "my-uuid"),
            Some("Müller, Klaus".to_string())
        );
    }

    #[test]
    fn last_message_from_me_returns_none() {
        let conv = serde_json::json!({
            "lastMessage": {
                "imdisplayname": "Baesler, Manuel",
                "from": "https://teams.microsoft.com/api/chatsvc/de/v1/users/ME/contacts/8:orgid:my-uuid"
            }
        });
        assert_eq!(resolve_name_from_last_message(&conv, "my-uuid"), None);
    }

    #[test]
    fn last_message_empty_name_returns_none() {
        let conv = serde_json::json!({
            "lastMessage": {
                "imdisplayname": "",
                "from": "https://teams.microsoft.com/api/chatsvc/de/v1/users/ME/contacts/8:orgid:other-uuid"
            }
        });
        assert_eq!(resolve_name_from_last_message(&conv, "my-uuid"), None);
    }

    #[test]
    fn no_last_message_returns_none() {
        let conv = serde_json::json!({});
        assert_eq!(resolve_name_from_last_message(&conv, "my-uuid"), None);
    }

    // --- parse_federated_profiles tests ---

    #[test]
    fn parse_federated_profiles_extracts_names() {
        let json = serde_json::json!({
            "value": [
                {
                    "mri": "8:orgid:aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
                    "displayName": "Mustermann, Max",
                    "email": "max.mustermann@example.com",
                    "type": "Federated"
                },
                {
                    "mri": "8:orgid:11111111-2222-3333-4444-555555555555",
                    "displayName": "Doe, Jane (Dept X/Unit Y)",
                    "email": "jane.doe@example.com",
                    "type": "Federated"
                }
            ]
        });
        let result = parse_federated_profiles(&serde_json::to_string(&json).unwrap());
        assert_eq!(result.len(), 2);
        assert_eq!(result.get("8:orgid:aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee").unwrap(), "Mustermann, Max");
        assert_eq!(result.get("8:orgid:11111111-2222-3333-4444-555555555555").unwrap(), "Doe, Jane (Dept X/Unit Y)");
    }

    #[test]
    fn parse_federated_profiles_skips_empty_names() {
        let json = serde_json::json!({
            "value": [
                { "mri": "8:orgid:aaa", "displayName": "" },
                { "mri": "8:orgid:bbb", "displayName": "Good Name" }
            ]
        });
        let result = parse_federated_profiles(&serde_json::to_string(&json).unwrap());
        assert_eq!(result.len(), 1);
        assert_eq!(result.get("8:orgid:bbb").unwrap(), "Good Name");
    }

    #[test]
    fn parse_federated_profiles_empty_response() {
        let json = serde_json::json!({ "value": [] });
        let result = parse_federated_profiles(&serde_json::to_string(&json).unwrap());
        assert!(result.is_empty());
    }

    #[test]
    fn parse_federated_profiles_invalid_json() {
        let result = parse_federated_profiles("not json");
        assert!(result.is_empty());
    }
}
