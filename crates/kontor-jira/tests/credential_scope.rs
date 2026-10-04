//! Synthetic ports only: installer and live connector use the same scoped address.
use kontor_accounts::{KeychainBackend, KeychainFailure, KeychainTarget, KeychainWriter};
use kontor_core::id::{ExternalId, ProjectId, RealmId};
use kontor_jira::{JiraConnectors, JiraCredentialScope, install_credentials_with};
use secrecy::{ExposeSecret, SecretString};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[derive(Default)]
struct FakeKeychain(Mutex<BTreeMap<(String, String), SecretString>>);
impl KeychainBackend for FakeKeychain {
    fn secret(&self, target: &KeychainTarget) -> Result<SecretString, KeychainFailure> {
        self.0
            .lock()
            .unwrap()
            .get(&(target.service().to_owned(), target.account().to_owned()))
            .cloned()
            .ok_or(KeychainFailure::NotFound)
    }
}
impl KeychainWriter for FakeKeychain {
    fn set_secret(
        &self,
        target: &KeychainTarget,
        secret: &SecretString,
    ) -> Result<(), KeychainFailure> {
        self.0.lock().unwrap().insert(
            (target.service().to_owned(), target.account().to_owned()),
            secret.clone(),
        );
        Ok(())
    }
    fn delete_secret(&self, target: &KeychainTarget) -> Result<(), KeychainFailure> {
        self.0
            .lock()
            .unwrap()
            .remove(&(target.service().to_owned(), target.account().to_owned()));
        Ok(())
    }
}
fn configure(root: &std::path::Path, endpoint: &str, project: ProjectId) {
    std::fs::write(root.join("jira.json"),serde_json::to_vec(&serde_json::json!({"schema_version":1,"projects":[{"project_id":project,"endpoint":endpoint,"project_key":"ASMA","credential_alias":"work"}]})).unwrap()).unwrap();
}
#[tokio::test]
async fn another_root_with_the_same_alias_and_copied_realm_cannot_replace_a_running_connectors_credential()
 {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/ASMA-1/comment"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            serde_json::json!({"startAt":0,"maxResults":100,"total":0,"comments":[]}),
        ))
        .expect(3)
        .mount(&server)
        .await;
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let realm = RealmId::generate();
    let project = ProjectId::generate();
    configure(a.path(), &server.uri(), project);
    configure(b.path(), &server.uri(), project);
    let backend = Arc::new(FakeKeychain::default());
    let old =
        SecretString::from(r#"{"email":"old@example.test","api_token":"old-synthetic-token"}"#);
    install_credentials_with(
        &JiraCredentialScope::at(a.path(), realm).unwrap(),
        "work",
        old.clone(),
        backend.as_ref(),
    )
    .unwrap();
    let running = JiraConnectors::read_with_keychain(a.path(), realm, backend.clone()).unwrap();
    let issue = ExternalId::parse("ASMA-1").unwrap();
    running
        .for_project(project)
        .unwrap()
        .comments(&issue)
        .await
        .unwrap();
    let new =
        SecretString::from(r#"{"email":"new@example.test","api_token":"new-synthetic-token"}"#);
    install_credentials_with(
        &JiraCredentialScope::at(b.path(), realm).unwrap(),
        "work",
        new,
        backend.as_ref(),
    )
    .unwrap();
    running
        .for_project(project)
        .unwrap()
        .comments(&issue)
        .await
        .unwrap();
    let mut relative = std::path::PathBuf::new();
    for _ in std::env::current_dir().unwrap().components().skip(1) {
        relative.push("..");
    }
    relative.push(a.path().strip_prefix("/").unwrap());
    let same_root = JiraConnectors::read_with_keychain(&relative, realm, backend.clone()).unwrap();
    same_root
        .for_project(project)
        .unwrap()
        .comments(&issue)
        .await
        .unwrap();
    let requests = server.received_requests().await.unwrap();
    assert_eq!(
        requests[0].headers.get("authorization"),
        requests[2].headers.get("authorization"),
        "serving through a relative root must use the same canonical credential address"
    );
    assert_eq!(
        requests[0].headers.get("authorization"),
        requests[1].headers.get("authorization"),
        "other root must not change the running consumer"
    );
    assert_eq!(
        backend.0.lock().unwrap().len(),
        2,
        "same alias has two distinct scoped entries"
    );
    assert!(
        backend
            .0
            .lock()
            .unwrap()
            .values()
            .any(|v| v.expose_secret() == old.expose_secret())
    );
}
#[tokio::test]
async fn connector_refuses_a_legacy_global_alias_instead_of_falling_back_to_it() {
    let server = MockServer::start().await;
    let root = tempfile::tempdir().unwrap();
    let project = ProjectId::generate();
    configure(root.path(), &server.uri(), project);
    let backend = Arc::new(FakeKeychain::default());
    backend.0.lock().unwrap().insert(
        ("kontor-jira".to_owned(), "work".to_owned()),
        SecretString::from(
            r#"{"email":"legacy@example.test","api_token":"legacy-synthetic-token"}"#,
        ),
    );
    let connectors =
        JiraConnectors::read_with_keychain(root.path(), RealmId::generate(), backend).unwrap();
    let error = connectors
        .for_project(project)
        .unwrap()
        .comments(&ExternalId::parse("ASMA-1").unwrap())
        .await
        .unwrap_err();
    assert!(!format!("{error:?}").contains("legacy-synthetic"));
    assert!(server.received_requests().await.unwrap().is_empty());
}
