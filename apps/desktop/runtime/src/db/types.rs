use serde::{Deserialize, Deserializer, Serialize};

#[derive(Deserialize)]
#[serde(untagged)]
enum DBProjectIdsField {
    One(String),
    Many(Vec<String>),
}

fn deserialize_project_ids<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(
        match Option::<DBProjectIdsField>::deserialize(deserializer)? {
            Some(DBProjectIdsField::One(project_id)) => vec![project_id],
            Some(DBProjectIdsField::Many(project_ids)) => project_ids,
            None => Vec::new(),
        },
    )
}

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
    pub project_ids: Vec<String>,
    pub name: String,
    pub engine: String,
    pub endpoint: String,
    pub database: String,
    pub environment: String,
    pub group: Option<String>,
    pub read_only: bool,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DBConnectionProfile {
    pub id: String,
    /// `projectId` is accepted so existing profile files migrate without user action.
    #[serde(
        default,
        alias = "projectId",
        deserialize_with = "deserialize_project_ids"
    )]
    pub project_ids: Vec<String>,
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
    pub environment: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default)]
    pub read_only: bool,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DBProfileUpsertRequest {
    pub id: Option<String>,
    #[serde(
        default,
        alias = "projectId",
        deserialize_with = "deserialize_project_ids"
    )]
    pub project_ids: Vec<String>,
    pub name: String,
    pub engine: String,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub database: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub ssl_mode: Option<String>,
    #[serde(default)]
    pub environment: Option<String>,
    #[serde(default)]
    pub group: Option<String>,
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
