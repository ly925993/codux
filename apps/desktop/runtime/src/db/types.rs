use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DBSummary {
    pub project_id: Option<String>,
    pub profiles: Vec<DBProfileSummary>,
    pub wrapper_available: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DBProfileSummary {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub engine: String,
    pub endpoint: String,
    pub database: String,
    pub read_only: bool,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DBConnectionProfile {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub engine: String,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub ssl_mode: String,
    #[serde(default)]
    pub read_only: bool,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DBProfileUpsertRequest {
    pub id: Option<String>,
    pub project_id: String,
    pub name: String,
    pub engine: String,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub database: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub ssl_mode: Option<String>,
    #[serde(default)]
    pub read_only: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DBProfilesSnapshot {
    pub project_id: Option<String>,
    pub profiles: Vec<DBConnectionProfile>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DBQueryResult {
    pub ok: bool,
    pub message: String,
}
