use super::types::{DBConnectionProfile, DBQueryResult};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use uuid::Uuid;

pub(super) fn write_test_profile_file(profile: &DBConnectionProfile) -> Result<PathBuf, String> {
    let path = std::env::temp_dir().join(format!("codux-db-profile-test-{}.json", Uuid::new_v4()));
    let body = serde_json::to_vec_pretty(&json!([profile])).map_err(|error| error.to_string())?;
    write_private_file(&path, &body)?;
    Ok(path)
}

#[cfg(unix)]
fn write_private_file(path: &Path, data: &[u8]) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| error.to_string())?;
    file.write_all(data).map_err(|error| error.to_string())
}

#[cfg(not(unix))]
fn write_private_file(path: &Path, data: &[u8]) -> Result<(), String> {
    fs::write(path, data).map_err(|error| error.to_string())
}

pub(super) fn run_db_test_command(
    wrapper: &Path,
    profile: &DBConnectionProfile,
    profiles_file: &Path,
) -> Result<DBQueryResult, String> {
    let output = Command::new(wrapper)
        .arg("--json")
        .arg(&profile.id)
        .arg("--")
        .arg(test_statement(profile))
        .env("CODUX_DB_PROFILES_FILE", profiles_file)
        .env(
            "CODUX_DB_PROJECT_ID",
            profile
                .project_ids
                .first()
                .map(String::as_str)
                .unwrap_or(""),
        )
        .output()
        .map_err(|error| format!("failed to run codux-db test command: {error}"))?;
    if output.status.success() {
        Ok(DBQueryResult {
            ok: true,
            message: format!(
                "Database connection test succeeded: {} row(s).",
                db_test_row_count(&output.stdout).unwrap_or(1)
            ),
        })
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let message = [stderr.trim(), stdout.trim()]
            .into_iter()
            .find(|value| !value.is_empty())
            .unwrap_or("codux-db test command failed")
            .to_string();
        Err(message)
    }
}

fn test_statement(profile: &DBConnectionProfile) -> &'static str {
    match profile.engine.as_str() {
        "sqlite" | "postgres" | "mysql" => "SELECT 1 AS ok",
        _ => "SELECT 1 AS ok",
    }
}

fn db_test_row_count(stdout: &[u8]) -> Option<usize> {
    let root: serde_json::Value = serde_json::from_slice(stdout).ok()?;
    root.get("rowCount")
        .and_then(serde_json::Value::as_u64)
        .map(|value| value as usize)
}
