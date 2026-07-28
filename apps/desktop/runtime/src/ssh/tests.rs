use super::helpers::{render_ssh_launch_context_for_profiles, sanitize_request};
use super::*;
use serde_json::Value;
use std::fs;
use std::sync::Mutex;
use uuid::Uuid;

fn profile_with_secret() -> SSHConnectionProfile {
    SSHConnectionProfile {
        id: "profile-1".to_string(),
        name: "Production".to_string(),
        host: "example.com".to_string(),
        port: 2222,
        username: "root".to_string(),
        credential_kind: "password".to_string(),
        private_key_path: "/Users/me/.ssh/id_ed25519".to_string(),
        updated_at: 1,
        password: Some("secret-password".to_string()),
        key_passphrase: Some("secret-passphrase".to_string()),
    }
}

#[test]
fn password_profiles_require_password() {
    let result = sanitize_request(SSHProfileUpsertRequest {
        id: None,
        name: "Production".to_string(),
        host: "example.com".to_string(),
        port: 22,
        username: "root".to_string(),
        credential_kind: "password".to_string(),
        private_key_path: None,
        password: None,
        key_passphrase: None,
    });
    assert!(result.is_err());
}

#[test]
fn launch_context_lists_profiles_without_secrets() {
    let mut profiles = vec![profile_with_secret()];
    let context = render_ssh_launch_context_for_profiles(&mut profiles, None).unwrap();
    assert!(context.contains("codux-ssh list"));
    assert!(context.contains("codux-ssh <profile-id>"));
    assert!(context.contains("codux-ssh <profile-id> -- '<remote-command>'"));
    assert!(context.contains("Always run `codux-ssh list` at the time of use"));
    assert!(context.contains("Do not grep the repository"));
    assert!(!context.contains("Production"));
    assert!(!context.contains("root@example.com:2222"));
    assert!(!context.contains("profile-1"));
    assert!(!context.contains("secret-password"));
    assert!(!context.contains("secret-passphrase"));
    assert!(!context.contains("/Users/me/.ssh/id_ed25519"));
}

#[test]
fn launch_command_only_references_profile_id() {
    let profile = profile_with_secret();
    let store = SSHStore {
        profiles: Mutex::new(vec![profile]),
        state_file: PathBuf::from("/tmp/codux-ssh-test.json"),
    };
    let command = store.launch_command("profile-1".to_string()).unwrap();
    assert!(command.command.contains("codux-ssh"));
    assert!(command.command.contains("profile-1"));
    assert!(!command.command.contains("secret-password"));
    assert!(!command.command.contains("secret-passphrase"));
}

#[cfg(unix)]
#[test]
fn ssh_test_profile_file_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let profile = profile_with_secret();
    let path = super::test_command::write_test_profile_file(&profile).unwrap();
    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    fs::remove_file(path).ok();
}

#[test]
fn ssh_store_uses_shared_config_document_snapshot() {
    let support_dir = std::env::temp_dir().join(format!("codux-ssh-store-{}", Uuid::new_v4()));
    fs::create_dir_all(&support_dir).unwrap();
    let store = SSHStore::from_support_dir(support_dir.clone());

    store
        .upsert(SSHProfileUpsertRequest {
            id: Some("profile-1".to_string()),
            name: "Production".to_string(),
            host: "example.com".to_string(),
            port: 2222,
            username: "root".to_string(),
            credential_kind: "password".to_string(),
            private_key_path: None,
            password: Some("secret-password".to_string()),
            key_passphrase: None,
        })
        .unwrap();

    let path = ssh_profiles_file_path_in(support_dir.clone());
    let raw = crate::config::ConfigDocumentStore::for_file(path).snapshot();
    let profiles = raw.as_array().expect("ssh profiles root array");
    assert_eq!(profiles.len(), 1);
    assert_eq!(
        profiles[0].get("id").and_then(Value::as_str),
        Some("profile-1")
    );

    fs::remove_dir_all(support_dir).ok();
}

#[test]
fn ssh_store_recovers_from_legacy_null_document() {
    let support_dir = std::env::temp_dir().join(format!("codux-ssh-null-{}", Uuid::new_v4()));
    fs::create_dir_all(&support_dir).unwrap();
    fs::write(ssh_profiles_file_path_in(support_dir.clone()), "null\n").unwrap();

    let service = SSHService::new(support_dir.clone(), support_dir.join("runtime-assets"));
    let summary = service.summary();
    assert!(summary.error.is_none());
    assert!(summary.profiles.is_empty());

    let store = SSHStore::from_support_dir(support_dir.clone());
    let snapshot = store
        .upsert(SSHProfileUpsertRequest {
            id: Some("profile-after-null".to_string()),
            name: "Recovered".to_string(),
            host: "example.com".to_string(),
            port: 22,
            username: "root".to_string(),
            credential_kind: "none".to_string(),
            private_key_path: None,
            password: None,
            key_passphrase: None,
        })
        .unwrap();
    assert_eq!(snapshot.profiles.len(), 1);
    assert_eq!(snapshot.profiles[0].id, "profile-after-null");

    fs::remove_dir_all(support_dir).ok();
}

#[cfg(not(windows))]
#[test]
fn codux_ssh_remote_command_exits_after_noninteractive_password_auth() {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::process::{Command, Stdio};

    let dir = std::env::temp_dir().join(format!("codux-ssh-noninteractive-{}", Uuid::new_v4()));
    let bin = dir.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let fake_ssh = bin.join("ssh");
    fs::write(
        &fake_ssh,
        "#!/bin/sh\n\
         for arg in \"$@\"; do\n\
           if [ \"$arg\" = \"-f\" ]; then\n\
             printf 'password: ' >&2\n\
             IFS= read -r _password\n\
             printf 'master-ready\\n' >> \"$CODUX_SSH_TEST_LOG\"\n\
             exit 0\n\
           fi\n\
         done\n\
         printf 'remote-ok\\n'\n\
         cat\n\
         exit 0\n",
    )
    .unwrap();
    let mut permissions = fs::metadata(&fake_ssh).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_ssh, permissions).unwrap();

    let profiles = dir.join("ssh_profiles.json");
    fs::write(
        &profiles,
        serde_json::json!([{
            "id": "profile-1",
            "name": "Test",
            "host": "example.com",
            "port": 22,
            "username": "root",
            "credentialKind": "password",
            "privateKeyPath": "",
            "password": "secret",
            "updatedAt": 1
        }])
        .to_string(),
    )
    .unwrap();

    let source_wrappers = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("runtime-assets/scripts/wrappers");
    let staged_wrappers = dir.join("runtime-assets/scripts/wrappers");
    let staged_bin = staged_wrappers.join("bin");
    fs::create_dir_all(&staged_bin).unwrap();
    let wrapper = staged_bin.join("codux-ssh");
    fs::copy(source_wrappers.join("bin/codux-ssh"), &wrapper).unwrap();
    fs::copy(
        source_wrappers.join("codux-ssh-expect.exp"),
        staged_wrappers.join("codux-ssh-expect.exp"),
    )
    .unwrap();
    let helper = staged_wrappers.join("codux-wrapper-helper");
    fs::write(
        &helper,
        "#!/bin/sh\n\
         if [ \"$1\" != \"--codux-wrapper-helper\" ]; then exit 64; fi\n\
         case \"$2\" in\n\
           ssh-profile-shell)\n\
             printf '%s\\n' 'ssh_password=secret' \"ssh_key_passphrase=''\" 'ssh_args=(ssh -p 22 -o ControlMaster=auto -o ControlPath=/tmp/codux-ssh-test-%r@%h:%p -o ControlPersist=300 root@example.com)'\n\
             ;;\n\
           ssh-list-profiles)\n\
             printf '%s\\n' '{\"profiles\":[{\"id\":\"profile-1\",\"name\":\"Test\",\"host\":\"example.com\",\"port\":22,\"username\":\"root\",\"endpoint\":\"root@example.com:22\",\"credential\":\"password\"}]}'\n\
             ;;\n\
           *) exit 64 ;;\n\
         esac\n",
    )
    .unwrap();
    for executable in [
        &fake_ssh,
        &wrapper,
        &helper,
        &staged_wrappers.join("codux-ssh-expect.exp"),
    ] {
        let mut permissions = fs::metadata(executable).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(executable, permissions).unwrap();
    }

    let log_file = dir.join("ssh.log");
    let mut child = Command::new("zsh")
        .arg(wrapper)
        .arg("profile-1")
        .arg("--")
        .arg("cat")
        .env("CODUX_SSH_TEST_LOG", &log_file)
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("CODUX_SSH_PROFILES_FILE", &profiles)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"upload-body\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();

    assert!(
        output.status.success(),
        "codux-ssh should exit after remote command, stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("remote-ok"), "{stdout}");
    assert!(stdout.contains("upload-body"), "{stdout}");
    assert!(!stdout.contains("secret"), "{stdout}");
    assert!(!stderr.contains("secret"), "{stderr}");
    assert_eq!(fs::read_to_string(log_file).unwrap(), "master-ready\n");

    fs::remove_dir_all(dir).ok();
}

#[cfg(not(windows))]
#[test]
fn codux_ssh_without_control_path_uses_expect_fallback_for_password_auth() {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    let dir = std::env::temp_dir().join(format!("codux-ssh-no-controlpath-{}", Uuid::new_v4()));
    let bin = dir.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let fake_ssh = bin.join("ssh");
    fs::write(
        &fake_ssh,
        "#!/bin/sh\n\
         for arg in \"$@\"; do\n\
           if [ \"$arg\" = \"-f\" ]; then\n\
             printf 'unexpected-master\\n'\n\
             exit 9\n\
           fi\n\
         done\n\
         printf 'password: ' >&2\n\
         IFS= read -r _password\n\
         printf 'fallback-ok\\n'\n\
         exit 0\n",
    )
    .unwrap();

    let profiles = dir.join("ssh_profiles.json");
    fs::write(
        &profiles,
        serde_json::json!([{
            "id": "profile-1",
            "name": "Test",
            "host": "example.com",
            "port": 22,
            "username": "root",
            "credentialKind": "password",
            "privateKeyPath": "",
            "password": "secret",
            "updatedAt": 1
        }])
        .to_string(),
    )
    .unwrap();

    let source_wrappers = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("runtime-assets/scripts/wrappers");
    let staged_wrappers = dir.join("runtime-assets/scripts/wrappers");
    let staged_bin = staged_wrappers.join("bin");
    fs::create_dir_all(&staged_bin).unwrap();
    let wrapper = staged_bin.join("codux-ssh");
    fs::copy(source_wrappers.join("bin/codux-ssh"), &wrapper).unwrap();
    fs::copy(
        source_wrappers.join("codux-ssh-expect.exp"),
        staged_wrappers.join("codux-ssh-expect.exp"),
    )
    .unwrap();
    let helper = staged_wrappers.join("codux-wrapper-helper");
    fs::write(
        &helper,
        "#!/bin/sh\n\
         if [ \"$1\" != \"--codux-wrapper-helper\" ]; then exit 64; fi\n\
         case \"$2\" in\n\
           ssh-profile-shell)\n\
             printf '%s\\n' 'ssh_password=secret' \"ssh_key_passphrase=''\" 'ssh_args=(ssh -p 22 root@example.com)'\n\
             ;;\n\
           *) exit 64 ;;\n\
         esac\n",
    )
    .unwrap();
    for executable in [
        &fake_ssh,
        &wrapper,
        &helper,
        &staged_wrappers.join("codux-ssh-expect.exp"),
    ] {
        let mut permissions = fs::metadata(executable).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(executable, permissions).unwrap();
    }

    let output = Command::new("zsh")
        .arg(wrapper)
        .arg("profile-1")
        .arg("--")
        .arg("echo fallback-ok")
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("CODUX_SSH_PROFILES_FILE", &profiles)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "codux-ssh fallback should succeed, stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("fallback-ok"), "{stdout}");
    assert!(!stdout.contains("unexpected-master"), "{stdout}");

    fs::remove_dir_all(dir).ok();
}

#[cfg(not(windows))]
#[test]
fn codux_ssh_scp_expands_remote_colon_path_and_invokes_scp() {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    let dir = std::env::temp_dir().join(format!("codux-ssh-scp-{}", Uuid::new_v4()));
    let bin = dir.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let args_file = dir.join("scp-args.txt");

    // Key auth (no password) → the wrapper exec's scp directly; the fake records argv.
    let fake_scp = bin.join("scp");
    fs::write(
        &fake_scp,
        format!(
            "#!/bin/sh\n\
             : > '{args}'\n\
             for arg in \"$@\"; do printf '%s\\n' \"$arg\" >> '{args}'; done\n\
             exit 0\n",
            args = args_file.display()
        ),
    )
    .unwrap();

    let key = dir.join("id_key");
    fs::write(&key, "KEY").unwrap();
    let profiles = dir.join("ssh_profiles.json");
    fs::write(
        &profiles,
        serde_json::json!([{
            "id": "profile-1",
            "name": "Test",
            "host": "example.com",
            "port": 2222,
            "username": "root",
            "credentialKind": "privateKey",
            "privateKeyPath": key.to_string_lossy(),
            "keyPassphrase": "",
            "updatedAt": 1
        }])
        .to_string(),
    )
    .unwrap();

    let source_wrappers = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("runtime-assets/scripts/wrappers");
    let staged_wrappers = dir.join("runtime-assets/scripts/wrappers");
    let staged_bin = staged_wrappers.join("bin");
    fs::create_dir_all(&staged_bin).unwrap();
    let wrapper = staged_bin.join("codux-ssh");
    fs::copy(source_wrappers.join("bin/codux-ssh"), &wrapper).unwrap();
    let helper = staged_wrappers.join("codux-wrapper-helper");
    fs::write(
        &helper,
        format!(
            "#!/bin/sh\n\
             if [ \"$1\" != \"--codux-wrapper-helper\" ]; then exit 64; fi\n\
             case \"$2\" in\n\
               scp-profile-shell)\n\
                 printf '%s\\n' \"ssh_password=''\" \"ssh_key_passphrase=''\" \"ssh_remote='root@example.com'\" 'scp_args=(scp -P 2222 -o StrictHostKeyChecking=accept-new -o ConnectTimeout=15 -i {key})'\n\
                 ;;\n\
               *) exit 64 ;;\n\
             esac\n",
            key = key.display()
        ),
    )
    .unwrap();

    for executable in [&fake_scp, &wrapper, &helper] {
        let mut permissions = fs::metadata(executable).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(executable, permissions).unwrap();
    }

    let output = Command::new("zsh")
        .arg(&wrapper)
        .arg("scp")
        .arg("profile-1")
        .arg(":/remote/file.log")
        .arg("./local.log")
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("CODUX_SSH_PROFILES_FILE", &profiles)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "codux-ssh scp should succeed, stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let recorded = fs::read_to_string(&args_file).unwrap_or_default();
    // Remote side expanded to user@host:path; local side untouched; key + hardening carried through.
    assert!(
        recorded.contains("root@example.com:/remote/file.log"),
        "{recorded}"
    );
    assert!(recorded.contains("./local.log"), "{recorded}");
    assert!(
        recorded.contains("StrictHostKeyChecking=accept-new"),
        "{recorded}"
    );
    assert!(recorded.lines().any(|line| line == "-i"), "{recorded}");

    fs::remove_dir_all(dir).ok();
}
