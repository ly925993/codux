use crate::runtime_paths::app_support_dir;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

mod helpers;
mod test_command;
#[cfg(test)]
mod tests;
mod types;

pub use helpers::{
    db_profiles_file_path, render_db_launch_context, render_db_launch_context_from_support_dir,
};
use helpers::{
    db_profiles_file_path_in, display_name, endpoint, load_profiles, sanitize_profiles,
    sanitize_request,
};
use test_command::{run_db_test_command, write_test_profile_file};
pub use types::*;

pub struct DBStore {
    document_store: Arc<crate::config::ConfigDocumentStore>,
}

impl DBStore {
    pub fn load_or_seed() -> Self {
        Self::from_support_dir(app_support_dir())
    }

    pub fn from_support_dir(support_dir: PathBuf) -> Self {
        let state_file = db_profiles_file_path_in(support_dir);
        let document_store = crate::config::ConfigDocumentStore::for_file(state_file);
        let loaded_profiles = profiles_from_document(&document_store.snapshot());
        if sanitize_profiles(loaded_profiles.clone()) != loaded_profiles {
            // Re-read under the shared document lock so migration cannot overwrite a concurrent save.
            let _ = document_store.update(|document| {
                let loaded_profiles = profiles_from_document(document);
                let profiles = sanitize_profiles(loaded_profiles.clone());
                if profiles != loaded_profiles {
                    *document =
                        serde_json::to_value(profiles).map_err(|error| error.to_string())?;
                }
                Ok(())
            });
        }
        Self { document_store }
    }

    pub fn snapshot(&self, project_id: Option<&str>) -> DBProfilesSnapshot {
        let profiles = profiles_from_document(&self.document_store.snapshot());
        snapshot_from_profiles(profiles, project_id)
    }

    pub fn upsert(&self, request: DBProfileUpsertRequest) -> Result<DBProfilesSnapshot, String> {
        let project_id = request.project_id.trim().to_string();
        let profile = sanitize_request(request)?;
        let profiles = self.document_store.update(|document| {
            let mut profiles = profiles_from_document(document);
            if let Some(index) = profiles
                .iter()
                .position(|item| item.project_id == profile.project_id && item.id == profile.id)
            {
                profiles[index] = profile;
            } else {
                profiles.push(profile);
            }
            *document = serde_json::to_value(&profiles).map_err(|error| error.to_string())?;
            Ok(profiles)
        })?;
        Ok(snapshot_from_profiles(profiles, Some(&project_id)))
    }

    pub fn delete(
        &self,
        project_id: &str,
        profile_id: String,
    ) -> Result<DBProfilesSnapshot, String> {
        let profiles = self.document_store.update(|document| {
            let mut profiles = profiles_from_document(document);
            profiles
                .retain(|profile| !(profile.project_id == project_id && profile.id == profile_id));
            *document = serde_json::to_value(&profiles).map_err(|error| error.to_string())?;
            Ok(profiles)
        })?;
        Ok(snapshot_from_profiles(profiles, Some(project_id)))
    }

    pub fn test_profile(
        &self,
        request: DBProfileUpsertRequest,
        wrapper_bin_dir: &Path,
    ) -> Result<DBQueryResult, String> {
        let profile = sanitize_request(request)?;
        let wrapper = db_wrapper_path(wrapper_bin_dir);
        if !wrapper.exists() {
            return Err("codux-db wrapper is not ready.".to_string());
        }
        let profiles_file = write_test_profile_file(&profile)?;
        let output = run_db_test_command(&wrapper, &profile, &profiles_file);
        let _ = std::fs::remove_file(&profiles_file);
        output
    }
}

fn profiles_from_document(document: &serde_json::Value) -> Vec<DBConnectionProfile> {
    serde_json::from_value(document.clone()).unwrap_or_default()
}

fn snapshot_from_profiles(
    profiles: Vec<DBConnectionProfile>,
    project_id: Option<&str>,
) -> DBProfilesSnapshot {
    let mut profiles = profiles
        .into_iter()
        .filter(|profile| {
            project_id
                .map(|project_id| profile.project_id == project_id)
                .unwrap_or(true)
        })
        .collect::<Vec<_>>();
    profiles.sort_by(|left, right| {
        display_name(left)
            .to_lowercase()
            .cmp(&display_name(right).to_lowercase())
    });
    DBProfilesSnapshot {
        project_id: project_id.map(str::to_string),
        profiles,
    }
}

pub struct DBService {
    profiles_path: PathBuf,
    wrapper_path: PathBuf,
    project_id: Option<String>,
}

impl DBService {
    pub fn new(support_dir: PathBuf, runtime_assets: PathBuf, project_id: Option<String>) -> Self {
        Self {
            profiles_path: support_dir.join("db_profiles.json"),
            wrapper_path: db_wrapper_path(runtime_assets),
            project_id,
        }
    }

    pub fn summary(&self) -> DBSummary {
        let wrapper_available = self.wrapper_path.is_file();
        let profiles = load_profiles(&self.profiles_path).unwrap_or_default();
        let mut profiles = sanitize_profiles(profiles)
            .into_iter()
            .filter(|profile| {
                self.project_id
                    .as_deref()
                    .map(|project_id| profile.project_id == project_id)
                    .unwrap_or(false)
            })
            .map(|profile| DBProfileSummary {
                name: display_name(&profile),
                endpoint: endpoint(&profile),
                id: profile.id,
                project_id: profile.project_id,
                database: profile.database,
                engine: profile.engine,
                read_only: profile.read_only,
                updated_at: profile.updated_at,
            })
            .collect::<Vec<_>>();
        profiles.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
        DBSummary {
            project_id: self.project_id.clone(),
            profiles,
            wrapper_available,
            error: None,
        }
    }
}

pub fn db_wrapper_path(runtime_assets: impl AsRef<Path>) -> PathBuf {
    #[cfg(windows)]
    {
        let runtime_assets = runtime_assets.as_ref();
        if runtime_assets.ends_with("bin") {
            runtime_assets.join("codux-db.ps1")
        } else {
            runtime_assets.join("scripts/wrappers/bin/codux-db.ps1")
        }
    }
    #[cfg(not(windows))]
    {
        let runtime_assets = runtime_assets.as_ref();
        if runtime_assets.ends_with("bin") {
            runtime_assets.join("codux-db")
        } else {
            runtime_assets.join("scripts/wrappers/bin/codux-db")
        }
    }
}
