use super::*;

fn snapshot() -> PublicKeySnapshot {
    PublicKeySnapshot {
        revision: 7,
        keys: vec![PublicKeyEntry {
            issuer: ExternalId::parse("test-issuer").expect("issuer"),
            key_id: ExternalId::parse("key-1").expect("key"),
            public_key_der: vec![1, 2, 3], // Deliberately not cryptographic DER.
            not_before: 100,
            expires_at: 200,
            revoked: true,
        }],
    }
}

fn value() -> serde_json::Value {
    serde_json::from_slice(&encode(&snapshot()).expect("encode")).expect("JSON")
}

#[test]
fn roundtrip_preserves_public_fields_without_establishing_trust_or_der_validity() {
    let original = snapshot();
    let bytes = encode(&original).expect("encode");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&bytes).expect("JSON")["schema_version"],
        1
    );
    let decoded = decode(&bytes).expect("decode untrusted config");
    assert_eq!(decoded.revision, original.revision);
    let key = &decoded.keys[0];
    assert_eq!(key.issuer, original.keys[0].issuer);
    assert_eq!(key.key_id, original.keys[0].key_id);
    assert_eq!(key.public_key_der, vec![1, 2, 3]);
    assert_eq!(
        (key.not_before, key.expires_at, key.revoked),
        (100, 200, true)
    );
    assert_eq!(encode(&decoded).expect("reencode"), bytes);
}

#[test]
fn input_ceiling_is_checked_before_parsing_and_exact_ceiling_is_accepted() {
    assert_eq!(
        decode(&vec![b'!'; MAX_SNAPSHOT_BYTES + 1]).expect_err("pre-parse size"),
        SnapshotCodecRefusal::Size
    );
    let mut bytes = encode(&snapshot()).expect("encode");
    bytes.resize(MAX_SNAPSHOT_BYTES, b' ');
    assert!(decode(&bytes).is_ok());
    assert_eq!(
        decode(b"").expect_err("empty payload"),
        SnapshotCodecRefusal::Payload
    );
}

#[test]
fn only_schema_version_one_is_accepted() {
    for version in [0, 2, u16::MAX] {
        let mut document = value();
        document["schema_version"] = version.into();
        assert_eq!(
            decode(&serde_json::to_vec(&document).expect("JSON")).expect_err("schema"),
            SnapshotCodecRefusal::Schema
        );
    }
}

#[test]
fn unknown_duplicate_missing_and_invalid_typed_fields_are_refused() {
    let edits: &[fn(&mut serde_json::Value)] = &[
        |v| v["unknown"] = true.into(),
        |v| v["keys"][0]["unknown"] = true.into(),
        |v| v["keys"][0]["issuer"] = "".into(),
        |v| v["keys"][0]["key_id"] = "".into(),
        |v| v["keys"][0]["public_key_der"] = "AAAA".into(),
        |v| v["keys"][0]["public_key_der"] = serde_json::json!([256]),
        |v| v["keys"][0]["not_before"] = (-1).into(),
        |v| v["keys"][0]["revoked"] = "false".into(),
        |v| {
            v.as_object_mut().expect("object").remove("revision");
        },
        |v| {
            v["keys"][0].as_object_mut().expect("key").remove("revoked");
        },
    ];
    for edit in edits {
        let mut document = value();
        edit(&mut document);
        assert_eq!(
            decode(&serde_json::to_vec(&document).expect("JSON")).expect_err("malformed"),
            SnapshotCodecRefusal::Payload
        );
    }
    let bytes = String::from_utf8(encode(&snapshot()).expect("encode")).expect("UTF8");
    for duplicate in [
        bytes.replacen("{", "{\"revision\":7,", 1),
        bytes.replacen("\"keys\":[{", "\"keys\":[{\"revoked\":true,", 1),
    ] {
        assert_eq!(
            decode(duplicate.as_bytes()).expect_err("duplicate field"),
            SnapshotCodecRefusal::Payload
        );
    }
}

#[test]
fn shared_snapshot_guards_refuse_encode_and_decode_without_divergence() {
    let edits: &[fn(&mut PublicKeySnapshot)] = &[
        |s| s.revision = 0,
        |s| s.keys.clear(),
        |s| s.keys.push(s.keys[0].clone()),
        |s| s.keys[0].public_key_der.clear(),
        |s| s.keys[0].public_key_der.resize(2_049, 0),
        |s| s.keys[0].expires_at = s.keys[0].not_before,
        |s| s.keys[0].expires_at = s.keys[0].not_before - 1,
    ];
    for edit in edits {
        let mut snapshot = snapshot();
        edit(&mut snapshot);
        assert_eq!(
            encode(&snapshot).expect_err("prevalidate encode"),
            SnapshotCodecRefusal::Snapshot
        );
        let wire = WireSnapshot {
            schema_version: 1,
            revision: snapshot.revision,
            keys: snapshot
                .keys
                .into_iter()
                .map(|k| WireKey {
                    issuer: k.issuer,
                    key_id: k.key_id,
                    public_key_der: k.public_key_der,
                    not_before: k.not_before,
                    expires_at: k.expires_at,
                    revoked: k.revoked,
                })
                .collect(),
        };
        assert_eq!(
            decode(&serde_json::to_vec(&wire).expect("wire")).expect_err("shared decode guard"),
            SnapshotCodecRefusal::Snapshot
        );
    }
}

#[test]
fn identifier_bounds_measure_utf8_bytes_for_both_issuer_and_key() {
    for issuer in [true, false] {
        let mut snapshot = snapshot();
        let exact = ExternalId::parse(&"é".repeat(128)).expect("valid core id, 256 UTF8 bytes");
        if issuer {
            snapshot.keys[0].issuer = exact;
        } else {
            snapshot.keys[0].key_id = exact;
        }
        assert!(decode(&encode(&snapshot).expect("exact byte limit")).is_ok());
        let over = ExternalId::parse(&"é".repeat(129)).expect("valid core id, 258 UTF8 bytes");
        if issuer {
            snapshot.keys[0].issuer = over;
        } else {
            snapshot.keys[0].key_id = over;
        }
        assert_eq!(
            encode(&snapshot).expect_err("byte limit before cloning"),
            SnapshotCodecRefusal::IdentifierBytes
        );
        let mut wire = value();
        wire["keys"][0][if issuer { "issuer" } else { "key_id" }] = "é".repeat(129).into();
        assert_eq!(
            decode(&serde_json::to_vec(&wire).expect("wire")).expect_err("decode byte limit"),
            SnapshotCodecRefusal::IdentifierBytes
        );
    }
}

#[test]
fn key_and_der_count_boundaries_keep_maximum_output_below_ceiling() {
    let mut snapshot = snapshot();
    snapshot.keys[0].public_key_der = vec![255; 2_048];
    for count in [1, 64] {
        snapshot.keys.resize(count, snapshot.keys[0].clone());
        for (index, key) in snapshot.keys.iter_mut().enumerate() {
            key.key_id = ExternalId::parse(&format!("key-{index}")).expect("key");
        }
        let bytes = encode(&snapshot).expect("valid boundary");
        assert!(bytes.len() <= MAX_SNAPSHOT_BYTES);
        assert_eq!(decode(&bytes).expect("decode").keys.len(), count);
    }
    snapshot.keys.push(snapshot.keys[0].clone());
    snapshot.keys[64].key_id = ExternalId::parse("key-64").expect("unique key");
    assert_eq!(
        encode(&snapshot).expect_err("65 keys"),
        SnapshotCodecRefusal::Snapshot
    );
    let mut wire = value();
    let key = wire["keys"][0].clone();
    wire["keys"] = serde_json::Value::Array(
        (0..65)
            .map(|i| {
                let mut k = key.clone();
                k["key_id"] = format!("key-{i}").into();
                k
            })
            .collect(),
    );
    assert_eq!(
        decode(&serde_json::to_vec(&wire).expect("wire")).expect_err("65 decoded keys"),
        SnapshotCodecRefusal::Snapshot
    );
}
