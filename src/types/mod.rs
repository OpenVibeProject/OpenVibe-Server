use serde::Deserialize;

#[derive(Deserialize)]
pub struct ConnectParams {
    pub id: String,
}

pub enum ClientType {
    Device,
    Mobile,
}