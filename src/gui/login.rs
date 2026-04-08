use iced::widget::{button, center, column, container, text};
use iced::{Element, Task};

use crate::auth::browser::{self, BrowserSession};
use crate::error::AppError;

pub struct LoginScreen {
    state: LoginState,
}

enum LoginState {
    WaitingForLogin,
    Refreshing,
    Error(String),
}

#[derive(Debug, Clone)]
pub enum Message {
    LoginResult(Result<Box<BrowserSession>, String>),
    Retry,
}

pub enum Action {
    Task(Task<Message>),
    LoggedIn(Box<BrowserSession>),
}

impl LoginScreen {
    pub fn new() -> (Self, Task<Message>) {
        let task = Task::perform(
            browser::login_with_browser(),
            |r: Result<BrowserSession, AppError>| {
                Message::LoginResult(
                    r.map(Box::new).map_err(|e| e.to_string()),
                )
            },
        );

        (
            Self { state: LoginState::WaitingForLogin },
            task,
        )
    }

    pub fn refreshing() -> Self {
        Self { state: LoginState::Refreshing }
    }

    pub fn update(&mut self, message: Message) -> Action {
        match message {
            Message::LoginResult(Ok(session)) => {
                Action::LoggedIn(session)
            }
            Message::LoginResult(Err(e)) => {
                self.state = LoginState::Error(e);
                Action::Task(Task::none())
            }
            Message::Retry => {
                let (new, task) = Self::new();
                *self = new;
                Action::Task(task)
            }
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let content = match &self.state {
            LoginState::Refreshing => {
                column![
                    text("Connecting to Microsoft Teams...").size(20),
                    text("Refreshing session...").size(14),
                ]
                .spacing(10)
                .align_x(iced::Alignment::Center)
            }
            LoginState::WaitingForLogin => {
                column![
                    text("Sign in to Teams").size(24),
                    text("").size(8),
                    text("A Chrome window has been opened.").size(14),
                    text("Please sign in to Teams there.").size(14),
                    text("").size(8),
                    text("This window will update automatically once you're logged in.").size(12),
                ]
                .spacing(4)
                .align_x(iced::Alignment::Center)
            }
            LoginState::Error(e) => {
                column![
                    text("Login Failed").size(20),
                    text("").size(4),
                    text(e.as_str()).size(12),
                    text("").size(8),
                    button(text("Retry")).on_press(Message::Retry),
                ]
                .spacing(10)
                .align_x(iced::Alignment::Center)
            }
        };
        center(container(content).padding(40).max_width(500)).into()
    }
}
