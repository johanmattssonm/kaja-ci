use serde::*;

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Action {
    pub action: String,
    pub command: Option<String>,
    pub folder: Option<String>,
    pub stdin: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct ActionResponse {
    pub action: String,
    pub status: Option<String>,
    pub stdout: Option<String>,
}
