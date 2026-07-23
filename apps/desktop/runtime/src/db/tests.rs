use super::helpers::{
    db_profiles_file_path_in, render_db_launch_context_for_profiles, sanitize_request,
};
use super::*;
use serde_json::Value;
use std::fs;
use std::sync::{Arc, Barrier};
use std::thread;
use uuid::Uuid;

fn profile_with_secret(project_id: &str) -> DBConnectionProfile {
    DBConnectionProfile {
        id: "db-1".to_string(),
        project_ids: vec![project_id.to_string()],
        name: "Production DB".to_string(),
        engine: "postgres".to_string(),
        host: "db.example.com".to_string(),
        port: 5432,
        database: "app".to_string(),
        username: "app_user".to_string(),
        password: Some("secret-password".to_string()),
        ssl_mode: "require".to_string(),
        environment: "production".to_string(),
        group: Some("Core".to_string()),
        read_only: true,
        updated_at: 1,
    }
}

#[test]
fn launch_context_lists_project_profiles_without_secrets() {
    let mut profiles = vec![
        profile_with_secret("project-a"),
        profile_with_secret("project-b"),
    ];
    profiles[1].id = "db-2".to_string();

    let context =
        render_db_launch_context_for_profiles(&mut profiles, Some("project-a"), None).unwrap();

    assert!(context.contains("codux-db list"));
    assert!(context.contains("codux-db <profile-id> -- '<statement>'"));
    assert!(context.contains("Always run `codux-db list` at the time of use"));
    assert!(context.contains("Do not grep the repository"));
    assert!(context.contains("cast them to text"));
    assert!(context.contains("column::text"));
    assert!(context.contains("CAST(column AS CHAR)"));
    assert!(!context.contains("Production DB"));
    assert!(!context.contains("db-1"));
    assert!(!context.contains("db-2"));
    assert!(!context.contains("secret-password"));
    assert!(!context.contains("app_user"));
}

#[cfg(unix)]
#[test]
fn db_test_profile_file_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let profile = profile_with_secret("project-a");
    let path = super::test_command::write_test_profile_file(&profile).unwrap();
    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    fs::remove_file(path).ok();
}

#[test]
fn db_store_filters_profiles_by_root_project() {
    let support_dir = std::env::temp_dir().join(format!("codux-db-store-{}", Uuid::new_v4()));
    fs::create_dir_all(&support_dir).unwrap();
    let store = DBStore::from_support_dir(support_dir.clone());

    store
        .upsert(DBProfileUpsertRequest {
            id: Some("db-1".to_string()),
            project_ids: vec!["project-a".to_string()],
            name: "A".to_string(),
            engine: "postgres".to_string(),
            host: Some("localhost".to_string()),
            port: Some(5432),
            database: "app_a".to_string(),
            username: Some("user_a".to_string()),
            password: Some("secret-a".to_string()),
            ssl_mode: Some("prefer".to_string()),
            environment: Some("development".to_string()),
            group: None,
            read_only: true,
        })
        .unwrap();
    store
        .upsert(DBProfileUpsertRequest {
            id: Some("db-2".to_string()),
            project_ids: vec!["project-b".to_string()],
            name: "B".to_string(),
            engine: "mysql".to_string(),
            host: Some("localhost".to_string()),
            port: Some(3306),
            database: "app_b".to_string(),
            username: Some("user_b".to_string()),
            password: Some("secret-b".to_string()),
            ssl_mode: Some("prefer".to_string()),
            environment: Some("testing".to_string()),
            group: None,
            read_only: false,
        })
        .unwrap();

    let project_a = store.snapshot(Some("project-a"));
    assert_eq!(project_a.profiles.len(), 1);
    assert_eq!(project_a.profiles[0].id, "db-1");

    let raw =
        crate::config::ConfigDocumentStore::for_file(db_profiles_file_path_in(support_dir.clone()))
            .snapshot();
    let profiles = raw.as_array().expect("db profiles root array");
    assert_eq!(profiles.len(), 2);
    assert_eq!(
        profiles[0].get("password").and_then(Value::as_str),
        Some("secret-a")
    );

    store
        .upsert(DBProfileUpsertRequest {
            id: Some("db-1".to_string()),
            project_ids: vec!["project-a".to_string()],
            name: "A updated".to_string(),
            engine: "postgres".to_string(),
            host: Some("localhost".to_string()),
            port: Some(5432),
            database: "app_a".to_string(),
            username: Some("user_a".to_string()),
            password: Some("secret-a".to_string()),
            ssl_mode: Some("prefer".to_string()),
            environment: Some("development".to_string()),
            group: None,
            read_only: true,
        })
        .unwrap();
    let updated = store.snapshot(Some("project-a"));
    assert_eq!(updated.profiles.len(), 1);
    assert_eq!(updated.profiles[0].name, "A updated");

    fs::remove_dir_all(support_dir).ok();
}

#[test]
fn legacy_project_id_migrates_without_losing_the_connection() {
    let support_dir = std::env::temp_dir().join(format!("codux-db-legacy-{}", Uuid::new_v4()));
    fs::create_dir_all(&support_dir).unwrap();
    let document_store =
        crate::config::ConfigDocumentStore::for_file(db_profiles_file_path_in(support_dir.clone()));
    document_store
        .save_snapshot(&serde_json::json!([{
            "id": "db-legacy",
            "projectId": "project-a",
            "name": "Legacy",
            "engine": "postgres",
            "host": "localhost",
            "port": 5432,
            "database": "app",
            "username": "app",
            "sslMode": "prefer",
            "readOnly": true,
            "updatedAt": 7
        }]))
        .unwrap();

    let snapshot = DBStore::from_support_dir(support_dir.clone()).snapshot(Some("project-a"));

    assert_eq!(snapshot.profiles.len(), 1);
    assert_eq!(snapshot.profiles[0].project_ids, vec!["project-a"]);
    assert_eq!(snapshot.profiles[0].environment, "unspecified");
    assert_eq!(snapshot.profiles[0].updated_at, 7);
    let migrated = document_store.snapshot();
    assert_eq!(migrated[0]["projectIds"], serde_json::json!(["project-a"]));
    assert!(migrated[0].get("projectId").is_none());

    fs::remove_dir_all(support_dir).ok();
}

#[test]
fn shared_profile_updates_are_visible_in_every_bound_project() {
    let support_dir = std::env::temp_dir().join(format!("codux-db-shared-{}", Uuid::new_v4()));
    fs::create_dir_all(&support_dir).unwrap();
    let store = DBStore::from_support_dir(support_dir.clone());
    let request = DBProfileUpsertRequest {
        id: Some("db-shared".to_string()),
        project_ids: vec!["project-a".to_string(), "project-b".to_string()],
        name: "Shared".to_string(),
        engine: "postgres".to_string(),
        host: Some("localhost".to_string()),
        port: Some(5432),
        database: "app".to_string(),
        username: Some("app".to_string()),
        password: Some("secret".to_string()),
        ssl_mode: Some("require".to_string()),
        environment: Some("production".to_string()),
        group: Some("Orders".to_string()),
        read_only: true,
    };
    store.upsert(request.clone()).unwrap();

    let mut updated_request = request;
    updated_request.name = "Shared updated".to_string();
    store.upsert(updated_request).unwrap();

    for project_id in ["project-a", "project-b"] {
        let snapshot = store.snapshot(Some(project_id));
        assert_eq!(snapshot.profiles.len(), 1);
        assert_eq!(snapshot.profiles[0].name, "Shared updated");
        assert_eq!(snapshot.profiles[0].environment, "production");
        assert_eq!(snapshot.profiles[0].group.as_deref(), Some("Orders"));
    }

    fs::remove_dir_all(support_dir).ok();
}

#[test]
fn updating_shared_projects_preserves_connection_fields_and_credentials() {
    let support_dir = std::env::temp_dir().join(format!("codux-db-rebind-{}", Uuid::new_v4()));
    fs::create_dir_all(&support_dir).unwrap();
    let store = DBStore::from_support_dir(support_dir.clone());
    store
        .upsert(DBProfileUpsertRequest {
            id: Some("db-shared".to_string()),
            project_ids: vec!["project-a".to_string(), "project-b".to_string()],
            name: "Production orders".to_string(),
            engine: "postgres".to_string(),
            host: Some("db.internal".to_string()),
            port: Some(5432),
            database: "orders".to_string(),
            username: Some("codux".to_string()),
            password: Some("secret".to_string()),
            ssl_mode: Some("require".to_string()),
            environment: Some("production".to_string()),
            group: Some("Orders".to_string()),
            read_only: true,
        })
        .unwrap();

    // Duplicate and blank IDs model repeated UI events without changing connection data.
    store
        .update_projects(
            "db-shared".to_string(),
            vec![
                "project-a".to_string(),
                " project-c ".to_string(),
                "project-c".to_string(),
                String::new(),
            ],
        )
        .unwrap();

    assert!(store.snapshot(Some("project-b")).profiles.is_empty());
    let snapshot = store.snapshot(Some("project-c"));
    let profile = &snapshot.profiles[0];
    assert_eq!(profile.project_ids, vec!["project-a", "project-c"]);
    assert_eq!(profile.name, "Production orders");
    assert_eq!(profile.host, "db.internal");
    assert_eq!(profile.password.as_deref(), Some("secret"));
    assert_eq!(profile.environment, "production");
    assert_eq!(profile.group.as_deref(), Some("Orders"));
    assert!(profile.read_only);

    fs::remove_dir_all(support_dir).ok();
}

#[test]
fn updating_shared_projects_rejects_an_empty_selection() {
    let support_dir = std::env::temp_dir().join(format!("codux-db-rebind-{}", Uuid::new_v4()));
    fs::create_dir_all(&support_dir).unwrap();
    let store = DBStore::from_support_dir(support_dir.clone());

    let error = store
        .update_projects("db-missing".to_string(), vec![String::new()])
        .unwrap_err();

    assert!(error.contains("at least one root project"));
    fs::remove_dir_all(support_dir).ok();
}

#[test]
fn removing_shared_profile_from_one_project_keeps_other_bindings() {
    let support_dir = std::env::temp_dir().join(format!("codux-db-detach-{}", Uuid::new_v4()));
    fs::create_dir_all(&support_dir).unwrap();
    let store = DBStore::from_support_dir(support_dir.clone());
    store
        .upsert(DBProfileUpsertRequest {
            id: Some("db-shared".to_string()),
            project_ids: vec!["project-a".to_string(), "project-b".to_string()],
            name: "Shared".to_string(),
            engine: "postgres".to_string(),
            host: Some("localhost".to_string()),
            port: Some(5432),
            database: "app".to_string(),
            username: Some("app".to_string()),
            password: None,
            ssl_mode: Some("prefer".to_string()),
            environment: Some("testing".to_string()),
            group: None,
            read_only: true,
        })
        .unwrap();

    store.delete("project-a", "db-shared".to_string()).unwrap();

    assert!(store.snapshot(Some("project-a")).profiles.is_empty());
    let project_b = store.snapshot(Some("project-b"));
    assert_eq!(project_b.profiles.len(), 1);
    assert_eq!(project_b.profiles[0].project_ids, vec!["project-b"]);

    fs::remove_dir_all(support_dir).ok();
}

#[test]
fn loading_profiles_preserves_their_update_timestamp() {
    let support_dir = std::env::temp_dir().join(format!("codux-db-load-{}", Uuid::new_v4()));
    fs::create_dir_all(&support_dir).unwrap();
    let profile = profile_with_secret("project-a");
    crate::config::ConfigDocumentStore::for_file(db_profiles_file_path_in(support_dir.clone()))
        .save_snapshot(&vec![profile.clone()])
        .unwrap();

    let snapshot = DBStore::from_support_dir(support_dir.clone()).snapshot(Some("project-a"));

    assert_eq!(snapshot.profiles, vec![profile]);
    fs::remove_dir_all(support_dir).ok();
}

#[test]
fn concurrent_db_stores_do_not_overwrite_each_other() {
    let support_dir = std::env::temp_dir().join(format!("codux-db-concurrent-{}", Uuid::new_v4()));
    fs::create_dir_all(&support_dir).unwrap();
    let stores = (0..8)
        .map(|_| DBStore::from_support_dir(support_dir.clone()))
        .collect::<Vec<_>>();
    let barrier = Arc::new(Barrier::new(stores.len()));

    let handles = stores
        .into_iter()
        .enumerate()
        .map(|(index, store)| {
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                store
                    .upsert(DBProfileUpsertRequest {
                        id: Some(format!("db-{index}")),
                        project_ids: vec!["project-a".to_string()],
                        name: format!("Database {index}"),
                        engine: "postgres".to_string(),
                        host: Some("localhost".to_string()),
                        port: Some(5432),
                        database: format!("app_{index}"),
                        username: Some("app".to_string()),
                        password: None,
                        ssl_mode: Some("prefer".to_string()),
                        environment: Some("development".to_string()),
                        group: None,
                        read_only: true,
                    })
                    .unwrap();
            })
        })
        .collect::<Vec<_>>();
    for handle in handles {
        handle.join().unwrap();
    }

    let snapshot = DBStore::from_support_dir(support_dir.clone()).snapshot(Some("project-a"));
    assert_eq!(snapshot.profiles.len(), 8);

    // Flush the debounced writer and verify the same complete snapshot reached disk.
    crate::config::flush_all_config_writes();
    let persisted: Vec<DBConnectionProfile> = serde_json::from_str(
        &fs::read_to_string(db_profiles_file_path_in(support_dir.clone())).unwrap(),
    )
    .unwrap();
    assert_eq!(persisted.len(), 8);

    fs::remove_dir_all(support_dir).ok();
}

#[test]
fn concurrent_upsert_and_delete_preserve_both_mutations() {
    let support_dir = std::env::temp_dir().join(format!("codux-db-mixed-{}", Uuid::new_v4()));
    fs::create_dir_all(&support_dir).unwrap();
    DBStore::from_support_dir(support_dir.clone())
        .upsert(DBProfileUpsertRequest {
            id: Some("db-delete".to_string()),
            project_ids: vec!["project-a".to_string()],
            name: "Delete me".to_string(),
            engine: "postgres".to_string(),
            host: Some("localhost".to_string()),
            port: Some(5432),
            database: "old_app".to_string(),
            username: Some("app".to_string()),
            password: None,
            ssl_mode: Some("prefer".to_string()),
            environment: Some("development".to_string()),
            group: None,
            read_only: true,
        })
        .unwrap();

    // Construct separate stores before either mutation to reproduce the former stale-snapshot race.
    let upsert_store = DBStore::from_support_dir(support_dir.clone());
    let delete_store = DBStore::from_support_dir(support_dir.clone());
    let barrier = Arc::new(Barrier::new(2));
    let upsert_barrier = Arc::clone(&barrier);
    let upsert = thread::spawn(move || {
        upsert_barrier.wait();
        upsert_store
            .upsert(DBProfileUpsertRequest {
                id: Some("db-new".to_string()),
                project_ids: vec!["project-a".to_string()],
                name: "New database".to_string(),
                engine: "postgres".to_string(),
                host: Some("localhost".to_string()),
                port: Some(5432),
                database: "new_app".to_string(),
                username: Some("app".to_string()),
                password: None,
                ssl_mode: Some("prefer".to_string()),
                environment: Some("development".to_string()),
                group: None,
                read_only: true,
            })
            .unwrap();
    });
    let delete = thread::spawn(move || {
        barrier.wait();
        delete_store
            .delete("project-a", "db-delete".to_string())
            .unwrap();
    });
    upsert.join().unwrap();
    delete.join().unwrap();

    let snapshot = DBStore::from_support_dir(support_dir.clone()).snapshot(Some("project-a"));
    assert_eq!(snapshot.profiles.len(), 1);
    assert_eq!(snapshot.profiles[0].id, "db-new");

    fs::remove_dir_all(support_dir).ok();
}

#[test]
fn sqlite_profiles_do_not_require_username_or_host() {
    let profile = sanitize_request(DBProfileUpsertRequest {
        id: None,
        project_ids: vec!["project-a".to_string()],
        name: "Local".to_string(),
        engine: "sqlite".to_string(),
        host: None,
        port: None,
        database: "/tmp/app.sqlite3".to_string(),
        username: None,
        password: None,
        ssl_mode: None,
        environment: Some("development".to_string()),
        group: None,
        read_only: true,
    })
    .unwrap();

    assert_eq!(profile.engine, "sqlite");
    assert!(profile.username.is_empty());
}

#[cfg(not(windows))]
#[test]
fn codux_db_wrapper_lists_project_profiles_without_secrets() {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    let dir = std::env::temp_dir().join(format!("codux-db-wrapper-list-{}", Uuid::new_v4()));
    let wrappers = dir.join("runtime-assets/scripts/wrappers");
    let bin = wrappers.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let wrapper = bin.join("codux-db");
    let helper = wrappers.join("codux-wrapper-helper");
    let profiles = dir.join("db_profiles.json");

    fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("runtime-assets/scripts/wrappers/bin/codux-db"),
        &wrapper,
    )
    .unwrap();
    fs::write(
        &helper,
        "#!/bin/sh\n\
         if [ \"$1\" != \"--codux-wrapper-helper\" ]; then exit 64; fi\n\
         if [ \"$2\" != \"db-list-profiles\" ]; then exit 64; fi\n\
         printf '%s\\n' '{\"profiles\":[{\"id\":\"db-1\",\"name\":\"Production\",\"engine\":\"postgres\",\"database\":\"app\",\"endpoint\":\"db.example.com:5432/app\",\"readOnly\":true}]}'\n",
    )
    .unwrap();
    fs::write(
        &profiles,
        serde_json::json!([{
            "id": "db-1",
            "projectId": "project-a",
            "name": "Production",
            "engine": "postgres",
            "host": "db.example.com",
            "port": 5432,
            "database": "app",
            "username": "app_user",
            "password": "secret-password",
            "readOnly": true,
            "updatedAt": 1
        }])
        .to_string(),
    )
    .unwrap();
    for executable in [&wrapper, &helper] {
        let mut permissions = fs::metadata(executable).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(executable, permissions).unwrap();
    }

    let output = Command::new("zsh")
        .arg(&wrapper)
        .arg("list")
        .env("CODUX_DB_PROFILES_FILE", &profiles)
        .env("CODUX_DB_PROJECT_ID", "project-a")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "codux-db list failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Production"), "{stdout}");
    assert!(!stdout.contains("secret-password"), "{stdout}");
    assert!(!stdout.contains("app_user"), "{stdout}");

    fs::remove_dir_all(dir).ok();
}
