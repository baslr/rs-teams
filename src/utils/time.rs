/// return current time in format "HH:MM:SS.mmm"
pub fn get_now_formatted() -> String {
    chrono::Local::now().format("%H:%M:%S%.3f").to_string()
}
