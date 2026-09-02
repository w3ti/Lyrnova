use std::collections::{BTreeMap, BTreeSet};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signature, VerifyingKey};
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{
    plugin_manifest::{ManifestOrigin, PluginManifest, PluginSource, validate_manifest},
    plugin_package::{PluginPackageAuthentication, PluginPackageDescriptor},
};

pub(crate) const CATALOG_SCHEMA_VERSION: u32 = 2;
pub(crate) const TRUST_SCHEMA_VERSION: u32 = 1;
pub(crate) const MAX_CATALOG_BYTES: usize = 512 * 1024;
const MAX_CATALOG_ENTRIES: usize = 256;
const MAX_PUBLISHER_KEYS: usize = 256;
const MAX_ROOT_KEYS: usize = 16;
const MAX_RELEASE_TAG_BYTES: usize = 128;
const MAX_FUTURE_VALIDITY_SECONDS: u64 = 366 * 24 * 60 * 60;
const CATALOG_DOMAIN: &[u8] = b"Lyrnova catalog v2\0";
const RELEASE_DOMAIN: &[u8] = b"Lyrnova plugin release v1\0";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub(crate) enum PluginCatalogError {
    InvalidCatalog,
    UnknownPlugin,
    DownloadUrlDenied,
    DownloadFailed,
    DownloadTooLarge,
    NoTrustedCatalogKeys,
    CatalogSignatureInvalid,
    PublisherSignatureInvalid,
    CatalogExpired,
    CatalogRollback,
    Io,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SignatureAlgorithm {
    Ed25519,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PublisherKeyStatus {
    Active,
    Revoked,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DetachedSignature {
    pub(crate) algorithm: SignatureAlgorithm,
    pub(crate) key_id: String,
    pub(crate) signature: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CatalogRootKey {
    pub(crate) algorithm: SignatureAlgorithm,
    pub(crate) key_id: String,
    pub(crate) public_key: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CatalogTrust {
    pub(crate) schema_version: u32,
    pub(crate) threshold: usize,
    pub(crate) keys: Vec<CatalogRootKey>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PublisherKey {
    pub(crate) publisher: String,
    pub(crate) algorithm: SignatureAlgorithm,
    pub(crate) key_id: String,
    pub(crate) public_key: String,
    pub(crate) status: PublisherKeyStatus,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CatalogPluginRelease {
    pub(crate) manifest: PluginManifest,
    pub(crate) descriptor: PluginPackageDescriptor,
    pub(crate) release_tag: String,
    pub(crate) publisher_signature: DetachedSignature,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SignedCatalog {
    pub(crate) schema_version: u32,
    pub(crate) version: u64,
    pub(crate) expires_at: u64,
    pub(crate) publisher_keys: Vec<PublisherKey>,
    pub(crate) entries: Vec<CatalogPluginRelease>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CatalogEnvelope {
    pub(crate) signed: SignedCatalog,
    pub(crate) signatures: Vec<DetachedSignature>,
}

#[derive(Clone, Debug)]
pub(crate) struct VerifiedCatalog {
    pub(crate) signed: SignedCatalog,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReleasePayload<'a> {
    schema_version: u32,
    manifest: &'a PluginManifest,
    descriptor: &'a PluginPackageDescriptor,
    release_tag: &'a str,
}

pub(crate) fn parse_trust(document: &str) -> Result<CatalogTrust, PluginCatalogError> {
    let trust: CatalogTrust =
        serde_json::from_str(document).map_err(|_| PluginCatalogError::InvalidCatalog)?;
    if trust.schema_version != TRUST_SCHEMA_VERSION
        || trust.threshold == 0
        || trust.keys.len() > MAX_ROOT_KEYS
    {
        return Err(PluginCatalogError::InvalidCatalog);
    }
    let mut ids = BTreeSet::new();
    for key in &trust.keys {
        validate_key(key.algorithm, &key.key_id, &key.public_key)?;
        if !ids.insert(key.key_id.as_str()) {
            return Err(PluginCatalogError::InvalidCatalog);
        }
    }
    if !trust.keys.is_empty() && trust.threshold > trust.keys.len() {
        return Err(PluginCatalogError::InvalidCatalog);
    }
    Ok(trust)
}

pub(crate) fn parse_embedded_catalog(
    document: &str,
    host_version: &Version,
    _now: u64,
) -> Result<VerifiedCatalog, PluginCatalogError> {
    let envelope = parse_envelope(document.as_bytes())?;
    verify_catalog_contents(envelope.signed, host_version, None)
}

pub(crate) fn parse_authenticated_catalog(
    bytes: &[u8],
    host_version: &Version,
    trust: &CatalogTrust,
    now: u64,
) -> Result<VerifiedCatalog, PluginCatalogError> {
    if trust.keys.is_empty() || trust.threshold > trust.keys.len() {
        return Err(PluginCatalogError::NoTrustedCatalogKeys);
    }
    let envelope = parse_envelope(bytes)?;
    verify_root_signatures(&envelope, trust)?;
    verify_catalog_contents(envelope.signed, host_version, Some(now))
}

fn parse_envelope(bytes: &[u8]) -> Result<CatalogEnvelope, PluginCatalogError> {
    if bytes.len() > MAX_CATALOG_BYTES {
        return Err(PluginCatalogError::InvalidCatalog);
    }
    serde_json::from_slice(bytes).map_err(|_| PluginCatalogError::InvalidCatalog)
}

fn verify_root_signatures(
    envelope: &CatalogEnvelope,
    trust: &CatalogTrust,
) -> Result<(), PluginCatalogError> {
    let payload = domain_payload(CATALOG_DOMAIN, &envelope.signed)?;
    let keys: BTreeMap<_, _> = trust.keys.iter().map(|key| (&key.key_id, key)).collect();
    let mut considered = BTreeSet::new();
    let mut verified = BTreeSet::new();
    for signature in &envelope.signatures {
        if signature.algorithm != SignatureAlgorithm::Ed25519
            || !considered.insert(signature.key_id.as_str())
        {
            continue;
        }
        let Some(key) = keys.get(&signature.key_id) else {
            continue;
        };
        if verify_signature(&key.public_key, &signature.signature, &payload).is_ok() {
            verified.insert(signature.key_id.as_str());
        }
    }
    if verified.len() < trust.threshold {
        return Err(PluginCatalogError::CatalogSignatureInvalid);
    }
    Ok(())
}

fn verify_catalog_contents(
    signed: SignedCatalog,
    host_version: &Version,
    now: Option<u64>,
) -> Result<VerifiedCatalog, PluginCatalogError> {
    if signed.schema_version != CATALOG_SCHEMA_VERSION
        || signed.version == 0
        || signed.expires_at == 0
        || signed.entries.len() > MAX_CATALOG_ENTRIES
        || signed.publisher_keys.len() > MAX_PUBLISHER_KEYS
    {
        return Err(PluginCatalogError::InvalidCatalog);
    }
    if let Some(now) = now {
        if signed.expires_at <= now {
            return Err(PluginCatalogError::CatalogExpired);
        }
        if signed.expires_at - now > MAX_FUTURE_VALIDITY_SECONDS {
            return Err(PluginCatalogError::InvalidCatalog);
        }
    }

    let mut publisher_keys = BTreeMap::new();
    for key in &signed.publisher_keys {
        validate_publisher(&key.publisher)?;
        validate_key(key.algorithm, &key.key_id, &key.public_key)?;
        if publisher_keys.insert(key.key_id.as_str(), key).is_some() {
            return Err(PluginCatalogError::InvalidCatalog);
        }
    }

    let mut ids = BTreeSet::new();
    for entry in &signed.entries {
        validate_manifest(&entry.manifest, host_version, ManifestOrigin::External)
            .map_err(|_| PluginCatalogError::InvalidCatalog)?;
        entry
            .descriptor
            .validate()
            .map_err(|_| PluginCatalogError::InvalidCatalog)?;
        let PluginSource::GithubRelease { asset, .. } = &entry.manifest.source else {
            return Err(PluginCatalogError::InvalidCatalog);
        };
        if asset != &entry.descriptor.asset
            || !valid_release_tag(&entry.release_tag)
            || !ids.insert(entry.manifest.id.as_str())
        {
            return Err(PluginCatalogError::InvalidCatalog);
        }
        let key = publisher_keys
            .get(entry.publisher_signature.key_id.as_str())
            .filter(|key| {
                key.publisher == entry.manifest.publisher
                    && key.status == PublisherKeyStatus::Active
                    && key.algorithm == entry.publisher_signature.algorithm
            })
            .ok_or(PluginCatalogError::PublisherSignatureInvalid)?;
        let payload =
            release_signing_payload(&entry.manifest, &entry.descriptor, &entry.release_tag)?;
        verify_signature(
            &key.public_key,
            &entry.publisher_signature.signature,
            &payload,
        )
        .map_err(|_| PluginCatalogError::PublisherSignatureInvalid)?;
    }
    Ok(VerifiedCatalog { signed })
}

pub(crate) fn authentication_for(
    catalog_version: u64,
    release: &CatalogPluginRelease,
) -> PluginPackageAuthentication {
    PluginPackageAuthentication {
        catalog_version,
        release_tag: release.release_tag.clone(),
        key_id: release.publisher_signature.key_id.clone(),
        signature: release.publisher_signature.signature.clone(),
    }
}

pub(crate) fn verify_installed_authentication(
    catalog: &VerifiedCatalog,
    manifest: &PluginManifest,
    descriptor: &PluginPackageDescriptor,
    authentication: &PluginPackageAuthentication,
) -> Result<(), PluginCatalogError> {
    if authentication.catalog_version == 0
        || authentication.catalog_version > catalog.signed.version
        || !valid_release_tag(&authentication.release_tag)
    {
        return Err(PluginCatalogError::PublisherSignatureInvalid);
    }
    let key = catalog
        .signed
        .publisher_keys
        .iter()
        .find(|key| {
            key.key_id == authentication.key_id
                && key.publisher == manifest.publisher
                && key.status == PublisherKeyStatus::Active
        })
        .ok_or(PluginCatalogError::PublisherSignatureInvalid)?;
    let payload = release_signing_payload(manifest, descriptor, &authentication.release_tag)?;
    verify_signature(&key.public_key, &authentication.signature, &payload)
        .map_err(|_| PluginCatalogError::PublisherSignatureInvalid)
}

pub(crate) fn rejects_release_downgrade(current: &VerifiedCatalog, next: &VerifiedCatalog) -> bool {
    let current_versions: BTreeMap<_, _> = current
        .signed
        .entries
        .iter()
        .map(|entry| (entry.manifest.id.as_str(), &entry.manifest.version))
        .collect();
    next.signed.entries.iter().any(|entry| {
        current_versions
            .get(entry.manifest.id.as_str())
            .is_some_and(|version| entry.manifest.version < **version)
    })
}

fn validate_key(
    algorithm: SignatureAlgorithm,
    key_id: &str,
    public_key: &str,
) -> Result<(), PluginCatalogError> {
    if algorithm != SignatureAlgorithm::Ed25519 {
        return Err(PluginCatalogError::InvalidCatalog);
    }
    let bytes = STANDARD
        .decode(public_key)
        .map_err(|_| PluginCatalogError::InvalidCatalog)?;
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| PluginCatalogError::InvalidCatalog)?;
    let key = VerifyingKey::from_bytes(&bytes).map_err(|_| PluginCatalogError::InvalidCatalog)?;
    if key.is_weak() || key_id != lower_hex(&Sha256::digest(bytes)) {
        return Err(PluginCatalogError::InvalidCatalog);
    }
    Ok(())
}

fn lower_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    output
}

fn verify_signature(
    public_key: &str,
    signature: &str,
    payload: &[u8],
) -> Result<(), PluginCatalogError> {
    let key_bytes: [u8; 32] = STANDARD
        .decode(public_key)
        .map_err(|_| PluginCatalogError::InvalidCatalog)?
        .try_into()
        .map_err(|_| PluginCatalogError::InvalidCatalog)?;
    let signature_bytes: [u8; 64] = STANDARD
        .decode(signature)
        .map_err(|_| PluginCatalogError::InvalidCatalog)?
        .try_into()
        .map_err(|_| PluginCatalogError::InvalidCatalog)?;
    let key =
        VerifyingKey::from_bytes(&key_bytes).map_err(|_| PluginCatalogError::InvalidCatalog)?;
    key.verify_strict(payload, &Signature::from_bytes(&signature_bytes))
        .map_err(|_| PluginCatalogError::CatalogSignatureInvalid)
}

fn validate_publisher(publisher: &str) -> Result<(), PluginCatalogError> {
    if publisher.is_empty()
        || publisher.len() > 160
        || !publisher
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(PluginCatalogError::InvalidCatalog);
    }
    Ok(())
}

fn valid_release_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() <= MAX_RELEASE_TAG_BYTES
        && tag.as_bytes()[0].is_ascii_alphanumeric()
        && tag
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn release_signing_payload(
    manifest: &PluginManifest,
    descriptor: &PluginPackageDescriptor,
    release_tag: &str,
) -> Result<Vec<u8>, PluginCatalogError> {
    domain_payload(
        RELEASE_DOMAIN,
        &ReleasePayload {
            schema_version: 1,
            manifest,
            descriptor,
            release_tag,
        },
    )
}

fn domain_payload(domain: &[u8], value: &impl Serialize) -> Result<Vec<u8>, PluginCatalogError> {
    let value = serde_json::to_value(value).map_err(|_| PluginCatalogError::InvalidCatalog)?;
    let mut output = Vec::with_capacity(domain.len() + 1024);
    output.extend_from_slice(domain);
    canonical_json(&value, &mut output)?;
    Ok(output)
}

fn canonical_json(value: &Value, output: &mut Vec<u8>) -> Result<(), PluginCatalogError> {
    match value {
        Value::Null => output.extend_from_slice(b"null"),
        Value::Bool(true) => output.extend_from_slice(b"true"),
        Value::Bool(false) => output.extend_from_slice(b"false"),
        Value::Number(number) if number.is_i64() || number.is_u64() => {
            output.extend_from_slice(number.to_string().as_bytes());
        }
        Value::Number(_) => return Err(PluginCatalogError::InvalidCatalog),
        Value::String(string) => output.extend_from_slice(
            serde_json::to_string(string)
                .map_err(|_| PluginCatalogError::InvalidCatalog)?
                .as_bytes(),
        ),
        Value::Array(values) => {
            output.push(b'[');
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    output.push(b',');
                }
                canonical_json(value, output)?;
            }
            output.push(b']');
        }
        Value::Object(values) => {
            output.push(b'{');
            let mut entries: Vec<_> = values.iter().collect();
            entries.sort_unstable_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));
            for (index, (key, value)) in entries.into_iter().enumerate() {
                if index != 0 {
                    output.push(b',');
                }
                output.extend_from_slice(
                    serde_json::to_string(key)
                        .map_err(|_| PluginCatalogError::InvalidCatalog)?
                        .as_bytes(),
                );
                output.push(b':');
                canonical_json(value, output)?;
            }
            output.push(b'}');
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};

    use super::*;
    use crate::plugin_manifest::{
        PluginCapability, PluginCompatibility, PluginKind, PluginPermission, PluginRuntime,
    };

    fn signing_key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn root_key(signing: &SigningKey) -> CatalogRootKey {
        let bytes = signing.verifying_key().to_bytes();
        CatalogRootKey {
            algorithm: SignatureAlgorithm::Ed25519,
            key_id: lower_hex(&Sha256::digest(bytes)),
            public_key: STANDARD.encode(bytes),
        }
    }

    fn publisher_key(signing: &SigningKey, status: PublisherKeyStatus) -> PublisherKey {
        let root = root_key(signing);
        PublisherKey {
            publisher: "example".into(),
            algorithm: root.algorithm,
            key_id: root.key_id,
            public_key: root.public_key,
            status,
        }
    }

    fn manifest(version: Version) -> PluginManifest {
        PluginManifest {
            schema_version: 1,
            id: "io.github.example.lyrnova.tool.example".into(),
            name: "Example".into(),
            description: "Curated external plugin used by trust tests.".into(),
            version,
            publisher: "example".into(),
            license: "GPL-3.0-only".into(),
            kind: PluginKind::Tool,
            compatibility: PluginCompatibility {
                lyrnova: ">=0.1.0, <0.2.0".parse().unwrap(),
                plugin_api: 1,
            },
            runtime: PluginRuntime::Process {
                entrypoint: "bin/example".into(),
                protocol_version: 1,
            },
            source: PluginSource::GithubRelease {
                repository: "https://github.com/example/lyrnova-example".into(),
                asset: "example.tar.zst".into(),
            },
            capabilities: vec![PluginCapability::Tasks],
            permissions: vec![
                PluginPermission::WorkspaceRead,
                PluginPermission::ProcessSpawn,
            ],
        }
    }

    fn signed_release(signing: &SigningKey, version: Version) -> CatalogPluginRelease {
        let manifest = manifest(version);
        let descriptor = PluginPackageDescriptor {
            asset: "example.tar.zst".into(),
            sha256: "a".repeat(64),
        };
        let release_tag = format!("v{}", manifest.version);
        let payload = release_signing_payload(&manifest, &descriptor, &release_tag).unwrap();
        CatalogPluginRelease {
            manifest,
            descriptor,
            release_tag,
            publisher_signature: DetachedSignature {
                algorithm: SignatureAlgorithm::Ed25519,
                key_id: publisher_key(signing, PublisherKeyStatus::Active).key_id,
                signature: STANDARD.encode(signing.sign(&payload).to_bytes()),
            },
        }
    }

    fn envelope(root: &SigningKey, publisher: &SigningKey, version: u64) -> CatalogEnvelope {
        let signed = SignedCatalog {
            schema_version: CATALOG_SCHEMA_VERSION,
            version,
            expires_at: 2_000_000,
            publisher_keys: vec![publisher_key(publisher, PublisherKeyStatus::Active)],
            entries: vec![signed_release(publisher, Version::new(0, 1, 0))],
        };
        let payload = domain_payload(CATALOG_DOMAIN, &signed).unwrap();
        CatalogEnvelope {
            signed,
            signatures: vec![DetachedSignature {
                algorithm: SignatureAlgorithm::Ed25519,
                key_id: root_key(root).key_id,
                signature: STANDARD.encode(root.sign(&payload).to_bytes()),
            }],
        }
    }

    fn verify(
        envelope: &CatalogEnvelope,
        root: &SigningKey,
    ) -> Result<VerifiedCatalog, PluginCatalogError> {
        parse_authenticated_catalog(
            &serde_json::to_vec(envelope).unwrap(),
            &Version::new(0, 1, 0),
            &CatalogTrust {
                schema_version: TRUST_SCHEMA_VERSION,
                threshold: 1,
                keys: vec![root_key(root)],
            },
            1_000_000,
        )
    }

    #[test]
    fn authenticates_root_and_delegated_publisher_signatures() {
        let root = signing_key(7);
        let publisher = signing_key(9);
        let catalog = verify(&envelope(&root, &publisher, 1), &root).unwrap();
        let release = &catalog.signed.entries[0];
        let authentication = authentication_for(catalog.signed.version, release);
        assert_eq!(
            verify_installed_authentication(
                &catalog,
                &release.manifest,
                &release.descriptor,
                &authentication,
            ),
            Ok(())
        );
    }

    #[test]
    fn tampering_and_revocation_fail_closed() {
        let root = signing_key(7);
        let publisher = signing_key(9);
        let mut tampered = envelope(&root, &publisher, 1);
        tampered.signed.entries[0].descriptor.sha256 = "b".repeat(64);
        let payload = domain_payload(CATALOG_DOMAIN, &tampered.signed).unwrap();
        tampered.signatures[0].signature = STANDARD.encode(root.sign(&payload).to_bytes());
        assert_eq!(
            verify(&tampered, &root).unwrap_err(),
            PluginCatalogError::PublisherSignatureInvalid
        );

        let mut revoked = envelope(&root, &publisher, 1);
        revoked.signed.publisher_keys[0].status = PublisherKeyStatus::Revoked;
        let payload = domain_payload(CATALOG_DOMAIN, &revoked.signed).unwrap();
        revoked.signatures[0].signature = STANDARD.encode(root.sign(&payload).to_bytes());
        assert_eq!(
            verify(&revoked, &root).unwrap_err(),
            PluginCatalogError::PublisherSignatureInvalid
        );
    }

    #[test]
    fn a_catalog_revocation_invalidates_an_already_installed_release() {
        let root = signing_key(7);
        let publisher = signing_key(9);
        let current = verify(&envelope(&root, &publisher, 1), &root).unwrap();
        let release = &current.signed.entries[0];
        let authentication = authentication_for(current.signed.version, release);

        let mut revoked_document = envelope(&root, &publisher, 2);
        revoked_document.signed.publisher_keys[0].status = PublisherKeyStatus::Revoked;
        revoked_document.signed.entries.clear();
        let payload = domain_payload(CATALOG_DOMAIN, &revoked_document.signed).unwrap();
        revoked_document.signatures[0].signature = STANDARD.encode(root.sign(&payload).to_bytes());
        let revoked = verify(&revoked_document, &root).unwrap();

        assert_eq!(
            verify_installed_authentication(
                &revoked,
                &release.manifest,
                &release.descriptor,
                &authentication,
            ),
            Err(PluginCatalogError::PublisherSignatureInvalid)
        );
    }

    #[test]
    fn threshold_expiration_and_downgrade_guards_are_enforced() {
        let root = signing_key(7);
        let publisher = signing_key(9);
        let document = envelope(&root, &publisher, 1);
        let wrong_root = signing_key(11);
        assert_eq!(
            verify(&document, &wrong_root).unwrap_err(),
            PluginCatalogError::CatalogSignatureInvalid
        );

        let threshold_trust = CatalogTrust {
            schema_version: TRUST_SCHEMA_VERSION,
            threshold: 2,
            keys: vec![root_key(&root), root_key(&wrong_root)],
        };
        let mut duplicated = document.clone();
        duplicated.signatures.push(duplicated.signatures[0].clone());
        assert_eq!(
            parse_authenticated_catalog(
                &serde_json::to_vec(&duplicated).unwrap(),
                &Version::new(0, 1, 0),
                &threshold_trust,
                1_000_000,
            )
            .unwrap_err(),
            PluginCatalogError::CatalogSignatureInvalid
        );
        let payload = domain_payload(CATALOG_DOMAIN, &document.signed).unwrap();
        let mut threshold_document = document.clone();
        threshold_document.signatures.push(DetachedSignature {
            algorithm: SignatureAlgorithm::Ed25519,
            key_id: root_key(&wrong_root).key_id,
            signature: STANDARD.encode(wrong_root.sign(&payload).to_bytes()),
        });
        assert!(
            parse_authenticated_catalog(
                &serde_json::to_vec(&threshold_document).unwrap(),
                &Version::new(0, 1, 0),
                &threshold_trust,
                1_000_000,
            )
            .is_ok()
        );

        let mut expired = envelope(&root, &publisher, 1);
        expired.signed.expires_at = 999_999;
        let payload = domain_payload(CATALOG_DOMAIN, &expired.signed).unwrap();
        expired.signatures[0].signature = STANDARD.encode(root.sign(&payload).to_bytes());
        assert_eq!(
            verify(&expired, &root).unwrap_err(),
            PluginCatalogError::CatalogExpired
        );

        let current = verify(&envelope(&root, &publisher, 1), &root).unwrap();
        let mut next_document = envelope(&root, &publisher, 2);
        next_document.signed.entries[0] = signed_release(&publisher, Version::new(0, 0, 9));
        let payload = domain_payload(CATALOG_DOMAIN, &next_document.signed).unwrap();
        next_document.signatures[0].signature = STANDARD.encode(root.sign(&payload).to_bytes());
        let next = verify(&next_document, &root).unwrap();
        assert!(rejects_release_downgrade(&current, &next));
    }

    #[test]
    fn canonical_json_is_independent_of_object_insertion_order() {
        let left: Value = serde_json::from_str(r#"{"b":2,"a":{"z":1,"x":0}}"#).unwrap();
        let right: Value = serde_json::from_str(r#"{"a":{"x":0,"z":1},"b":2}"#).unwrap();
        let mut left_bytes = Vec::new();
        let mut right_bytes = Vec::new();
        canonical_json(&left, &mut left_bytes).unwrap();
        canonical_json(&right, &mut right_bytes).unwrap();
        assert_eq!(left_bytes, right_bytes);
        assert_eq!(left_bytes, br#"{"a":{"x":0,"z":1},"b":2}"#);
    }
}
