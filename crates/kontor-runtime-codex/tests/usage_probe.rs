//! The live usage probe's request path, against a recorded server.
//!
//! The classification is unit-tested beside the code that does it. What this
//! suite exists for is the half a pure test cannot reach: that the token
//! actually travels as a bearer header, that a refusal is a refusal rather than
//! an empty payload, and that no refusal carries the endpoint or the token.
//!
//! The distinction in the third test is the one that matters operationally. A
//! transport failure must never be reported as a readable-but-empty answer,
//! because an empty answer classifies as `unknown`, and `unknown` blocks — so a
//! momentary network fault would silently park an account that had plenty of
//! room, which is the exact failure this whole probe was built to prevent.

use kontor_runtime::adapter::RuntimeError;
use kontor_runtime_codex::usage::{
    AUTH_FILE_NAME, CodexLiveUsageProbe, CodexUsageProbe, USAGE_ENDPOINT,
};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A config home holding one fixture credential.
fn home_with_token(token: &str) -> tempfile::TempDir {
    let home = tempfile::tempdir().expect("a temporary config home");
    std::fs::write(
        home.path().join(AUTH_FILE_NAME),
        format!(r#"{{"tokens":{{"access_token":"{token}"}}}}"#),
    )
    .expect("the fixture credential file is written");
    home
}

/// The verified Pro shape: one weekly window, no secondary.
fn usage_body() -> serde_json::Value {
    serde_json::json!({
        "rate_limits": {
            "primary": {
                "used_percent": 62.0,
                "window_minutes": 10080,
                "reset_at": 1_788_121_720_i64
            },
            "secondary": null
        }
    })
}

#[tokio::test]
async fn the_probe_sends_the_accounts_own_token_as_a_bearer_header() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/usage"))
        // The assertion *is* the matcher: if the header does not arrive exactly
        // like this, the mock does not match and the request 404s.
        .and(header("authorization", "Bearer tok-work-account"))
        .respond_with(ResponseTemplate::new(200).set_body_json(usage_body()))
        .mount(&server)
        .await;

    let home = home_with_token("tok-work-account");
    let probe =
        CodexLiveUsageProbe::against(reqwest::Client::new(), format!("{}/usage", server.uri()));
    let usage = probe
        .usage(&home.path().to_string_lossy())
        .await
        .expect("the recorded server answers");
    let window = usage.rate_limits.primary.expect("the weekly window");
    assert_eq!(window.window_minutes, 10080);
    assert_eq!(window.reset_at, Some(1_788_121_720));
}

#[tokio::test]
async fn two_homes_are_probed_as_two_accounts() {
    // The open provider question this probe was built to settle: whether a
    // second Codex login reports its own windows or shares the first's. The
    // mechanism has to keep them apart before the question can even be asked, so
    // each home's token must reach the endpoint as that home's token.
    let server = MockServer::start().await;
    for (token, used) in [("tok-work", 62.0), ("tok-personal", 3.0)] {
        Mock::given(method("GET"))
            .and(path("/usage"))
            .and(header("authorization", format!("Bearer {token}").as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "rate_limits": {
                    "primary": {
                        "used_percent": used,
                        "window_minutes": 10080,
                        "reset_at": 1_788_121_720_i64
                    }
                }
            })))
            .mount(&server)
            .await;
    }

    let endpoint = format!("{}/usage", server.uri());
    let probe = CodexLiveUsageProbe::against(reqwest::Client::new(), endpoint);
    let mut observed = Vec::new();
    for token in ["tok-work", "tok-personal"] {
        let home = home_with_token(token);
        let usage = probe
            .usage(&home.path().to_string_lossy())
            .await
            .expect("the recorded server answers");
        observed.push(usage.rate_limits.primary.expect("a window").used_percent);
    }
    assert_eq!(
        observed,
        vec![62.0, 3.0],
        "each home must be read as its own account, or the two logins are indistinguishable"
    );
}

#[tokio::test]
async fn a_refused_probe_is_a_transport_refusal_and_never_an_empty_payload() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/usage"))
        .respond_with(ResponseTemplate::new(401).set_body_string("stale token"))
        .mount(&server)
        .await;

    let home = home_with_token("tok-stale");
    let probe =
        CodexLiveUsageProbe::against(reqwest::Client::new(), format!("{}/usage", server.uri()));
    let error = probe
        .usage(&home.path().to_string_lossy())
        .await
        .expect_err("a 401 is a refusal");
    assert!(
        matches!(error, RuntimeError::Transport { .. }),
        "a channel failure must not be reported as a readable answer: {error:?}"
    );
    let rendered = error.to_string();
    for forbidden in ["tok-stale", "stale token", &server.uri()] {
        assert!(
            !rendered.contains(forbidden),
            "a refusal must carry a reason, never `{forbidden}`: {rendered}"
        );
    }
}

#[tokio::test]
async fn a_body_in_an_unexpected_shape_is_refused_rather_than_read_as_no_windows() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/usage"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>signed out</html>"))
        .mount(&server)
        .await;

    let home = home_with_token("tok-any");
    let probe =
        CodexLiveUsageProbe::against(reqwest::Client::new(), format!("{}/usage", server.uri()));
    assert!(
        matches!(
            probe.usage(&home.path().to_string_lossy()).await,
            Err(RuntimeError::Transport { .. })
        ),
        "an unreadable body is a transport fault, not an account with no windows"
    );
}

#[test]
fn the_default_endpoint_is_the_vendors_own() {
    assert_eq!(
        USAGE_ENDPOINT, "https://chatgpt.com/backend-api/wham/usage",
        "the probe reads the vendor's structured usage endpoint"
    );
}
