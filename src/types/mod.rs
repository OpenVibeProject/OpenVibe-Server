use serde::Deserialize;

#[derive(Deserialize)]
pub struct ConnectParams {
    pub id: String,
}

pub enum ClientType {
    Master,
    Slave,
}

impl std::fmt::Display for ClientType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientType::Master => write!(f, "Master"),
            ClientType::Slave => write!(f, "Slave"),
        }
    }
}