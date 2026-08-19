//! The shipped seat configurations say what they claim to say.
//!
//! A seat file is the only place the authority boundary is stated to an operator,
//! so a typo in one is a silently wrong privilege rather than a broken build. These
//! read the files that actually ship.

use std::path::PathBuf;

/// The directory the seat files live in.
fn seats() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("seats")
}

/// One seat file's `args` array, and the server name it configures.
fn seat(file: &str) -> (String, Vec<String>) {
    let text = std::fs::read_to_string(seats().join(file))
        .unwrap_or_else(|error| panic!("{file} is readable: {error}"));
    let document: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|error| panic!("{file} is JSON: {error}"));
    let servers = document["mcpServers"]
        .as_object()
        .unwrap_or_else(|| panic!("{file} declares mcpServers"));
    assert_eq!(
        servers.len(),
        1,
        "{file} configures exactly one server, because a seat is one authority"
    );
    let (name, server) = servers.iter().next().expect("one server");
    assert_eq!(
        server["command"], "kontor-mcp",
        "{file} runs the seat binary"
    );
    let args = server["args"]
        .as_array()
        .unwrap_or_else(|| panic!("{file} declares args"))
        .iter()
        .map(|value| {
            value
                .as_str()
                .unwrap_or_else(|| panic!("{file} passes text arguments"))
                .to_owned()
        })
        .collect();
    (name.clone(), args)
}

/// The value one flag carries.
fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|argument| argument == name)
        .and_then(|at| args.get(at + 1))
        .cloned()
}

#[test]
fn each_seat_selects_exactly_the_tier_it_is_named_for() {
    for (file, expected) in [
        ("paseo-lead.json", "admin"),
        ("worker.json", "operator"),
        ("reviewer.json", "observer"),
    ] {
        let (name, args) = seat(file);
        let tier = flag(&args, "--credential-tier")
            .unwrap_or_else(|| panic!("{file} names a credential tier"));
        assert_eq!(tier, expected, "{file} configures the wrong authority");
        // The spelling has to be one the binary accepts, or the seat fails to
        // start with a message nobody sees until they try it.
        kontor_mcp::CallerTier::parse(&tier)
            .unwrap_or_else(|error| panic!("{file} names a real tier: {error}"));
        assert!(
            name.starts_with("kontor-"),
            "{file} names its server after the control plane it reaches"
        );
        assert!(
            flag(&args, "--state-root").is_some(),
            "{file} names the realm to act on"
        );
    }
}

/// TEST-005: the worker seat is pinned to (operator, `worker` profile), and no
/// other seat names a profile. A profile in a seat file must be one the registry
/// declares — the name enforces nothing by itself; the binary refuses an unknown
/// one at startup, and this test catches it before anyone starts anything.
#[test]
fn the_worker_seat_serves_the_worker_profile_and_the_others_serve_none() {
    for (file, expected_tier, expected_profile) in [
        ("paseo-lead.json", "admin", None),
        ("worker.json", "operator", Some("worker")),
        ("reviewer.json", "observer", None),
    ] {
        let (_, args) = seat(file);
        assert_eq!(
            flag(&args, "--credential-tier").as_deref(),
            Some(expected_tier),
            "{file} configures the wrong authority"
        );
        let profile = flag(&args, "--serve-profile");
        assert_eq!(
            profile.as_deref(),
            expected_profile,
            "{file} names the wrong serve profile"
        );
        if let Some(profile) = profile {
            kontor_mcp::ServeProfile::find(&profile).unwrap_or_else(|| {
                panic!("{file} names `{profile}`, which the registry does not declare")
            });
        }
    }
}

#[test]
fn only_the_lead_seat_is_admin_scoped() {
    let admin_seats: Vec<&str> = ["paseo-lead.json", "worker.json", "reviewer.json"]
        .into_iter()
        .filter(|file| {
            let (_, args) = seat(file);
            flag(&args, "--credential-tier").as_deref() == Some("admin")
        })
        .collect();
    assert_eq!(
        admin_seats,
        vec!["paseo-lead.json"],
        "the admin credential belongs to exactly one seat"
    );
}

#[test]
fn no_seat_carries_a_secret_or_a_non_loopback_address() {
    for file in ["paseo-lead.json", "worker.json", "reviewer.json"] {
        let text = std::fs::read_to_string(seats().join(file)).expect("readable");
        let lowered = text.to_lowercase();
        for forbidden in [
            "bearer",
            "token",
            "secret",
            "password",
            "authorization",
            "api_key",
        ] {
            assert!(
                !lowered.contains(forbidden),
                "{file} carries something credential-shaped: a secret on argv is \
                 visible in every process listing on the machine"
            );
        }
        // A base URL is optional, and when present it must be loopback — the
        // binary refuses anything else, and a seat file that shipped one would be
        // a configuration nobody could start.
        let (_, args) = seat(file);
        if let Some(base_url) = flag(&args, "--base-url") {
            kontor_mcp::Endpoint::parse(&base_url)
                .unwrap_or_else(|error| panic!("{file} names a loopback realm: {error}"));
        }
    }
}

#[test]
fn the_seat_flags_are_the_ones_the_binary_declares() {
    // A seat file naming a flag the binary does not have fails at startup with a
    // clap error, which is exactly the failure an operator discovers last.
    for file in ["paseo-lead.json", "worker.json", "reviewer.json"] {
        let (_, args) = seat(file);
        for argument in args.iter().filter(|argument| argument.starts_with("--")) {
            assert!(
                matches!(
                    argument.as_str(),
                    "--state-root" | "--credential-tier" | "--base-url" | "--serve-profile"
                ),
                "{file} passes {argument}, which `kontor-mcp` does not declare"
            );
        }
    }
}
