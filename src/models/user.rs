/// User profile information
#[derive(Debug, Clone)]
pub struct User {
    pub id: String,
    pub display_name: String,
    pub email: Option<String>,
}
