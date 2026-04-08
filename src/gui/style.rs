use iced::Color;

/// Teams-inspired dark theme colors
pub struct TeamsDark;

impl TeamsDark {
    pub const BACKGROUND: Color = Color::from_rgb(0.13, 0.13, 0.13);
    pub const SIDEBAR_BG: Color = Color::from_rgb(0.11, 0.11, 0.11);
    pub const CHAT_BG: Color = Color::from_rgb(0.16, 0.16, 0.16);
    pub const INPUT_BG: Color = Color::from_rgb(0.20, 0.20, 0.20);
    pub const TEXT_PRIMARY: Color = Color::WHITE;
    pub const TEXT_SECONDARY: Color = Color::from_rgb(0.65, 0.65, 0.65);
    pub const TEXT_MUTED: Color = Color::from_rgb(0.45, 0.45, 0.45);
    pub const ACCENT: Color = Color::from_rgb(0.38, 0.45, 0.95);
    pub const SELECTED: Color = Color::from_rgb(0.22, 0.22, 0.28);
    pub const HOVER: Color = Color::from_rgb(0.19, 0.19, 0.19);
    pub const BUBBLE_ME: Color = Color::from_rgb(0.30, 0.35, 0.70);
    pub const BUBBLE_OTHER: Color = Color::from_rgb(0.22, 0.22, 0.22);
    pub const BORDER: Color = Color::from_rgb(0.25, 0.25, 0.25);
    pub const HEADER_BG: Color = Color::from_rgb(0.14, 0.14, 0.14);
}

pub const SIDEBAR_WIDTH: f32 = 280.0;
pub const SPACING_SM: f32 = 4.0;
pub const SPACING_MD: f32 = 8.0;
pub const SPACING_LG: f32 = 16.0;
