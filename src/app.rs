use iced::{Element, Subscription, Task};

use crate::api::client::GraphClient;
use crate::api::csa::CsaFolder;
use crate::gui::login::{self, LoginScreen};
use crate::gui::screen::{self, MainScreen};

pub struct App {
    screen: Screen,
}

enum Screen {
    Login(LoginScreen),
    Main(MainScreen),
}

#[derive(Debug, Clone)]
pub enum Message {
    Login(login::Message),
    Main(screen::Message),
}

pub fn boot() -> (App, Task<Message>) {
    let (login, task) = LoginScreen::new();
    (
        App { screen: Screen::Login(login) },
        task.map(Message::Login),
    )
}

pub fn update(app: &mut App, message: Message) -> Task<Message> {
    match message {
        Message::Login(msg) => {
            let Screen::Login(login) = &mut app.screen else {
                return Task::none();
            };
            match login.update(msg) {
                login::Action::Task(task) => task.map(Message::Login),
                login::Action::LoggedIn(session) => {
                    tracing::info!(
                        "Login successful: user={}, region={}",
                        session.display_name,
                        session.region
                    );

                    let client = GraphClient::new(
                        session.skype_spaces_token.clone(),
                        session.ic3_token.clone(),
                        session.graph_token.clone(),
                        session.csa_token.clone(),
                        session.region.clone(),
                        session.mt_region.clone(),
                        session.chat_service_url.clone(),
                    );

                    // Folders come from the CSA endpoint now, not from browser scraping
                    let initial_folders: Vec<CsaFolder> = vec![];
                    let folder_order: Vec<String> = vec![];

                    let (main, task) = MainScreen::new(
                        client,
                        session.display_name.clone(),
                        session.user_id.clone(),
                        initial_folders,
                        folder_order,
                    );
                    app.screen = Screen::Main(main);
                    task.map(Message::Main)
                }
            }
        }
        Message::Main(msg) => {
            let Screen::Main(main) = &mut app.screen else {
                return Task::none();
            };
            main.update(msg).map(Message::Main)
        }
    }
}

pub fn view(app: &App) -> Element<'_, Message> {
    match &app.screen {
        Screen::Login(login) => login.view().map(Message::Login),
        Screen::Main(main) => main.view().map(Message::Main),
    }
}

pub fn subscription(app: &App) -> Subscription<Message> {
    match &app.screen {
        Screen::Main(main) => main.subscription().map(Message::Main),
        _ => Subscription::none(),
    }
}

pub fn theme(_app: &App) -> iced::Theme {
    iced::Theme::Dark
}
