use iced::widget::{button, column, container, scrollable, text};
use iced::{Element, Length};

use crate::models::chat::Chat;

#[derive(Debug, Clone)]
pub enum Message {
    ChatSelected(String),
}

pub fn view<'a>(
    chats: &'a [Chat],
    selected_chat_id: Option<&'a str>,
) -> Element<'a, Message> {
    let items: Vec<Element<'a, Message>> = chats
        .iter()
        .map(|chat| {
            let _is_selected =
                selected_chat_id == Some(chat.id.as_str());

            let preview = chat
                .last_message_preview
                .as_deref()
                .unwrap_or("")
                .chars()
                .take(45)
                .collect::<String>();

            let item = container(
                column![
                    text(&chat.display_name).size(14),
                    text(preview).size(11),
                ]
                .spacing(2),
            )
            .padding(8)
            .width(Length::Fill);

            button(item)
                .on_press(Message::ChatSelected(chat.id.clone()))
                .width(Length::Fill)
                .into()
        })
        .collect();

    container(
        scrollable(
            column(items).spacing(2).width(Length::Fill),
        )
        .height(Length::Fill),
    )
    .width(260)
    .height(Length::Fill)
    .into()
}
