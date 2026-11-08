//! Release trust (DECISIONS.md D5): stable and nightly releases are accepted only with a `manifest.json` whose
//! minisign signature verifies against a key built into Tools and whose trusted comment names this app, tag and
//! channel. Test (rc) manifests are unsigned and reachable only through `ARCADE_TOOLS_CHANNEL_OVERRIDE=test`.
use crate::release::{versions_match, Asset, Channel, Release};
use crate::{Error, Result};
use arcade_link::release::{self as link, FileEntry, PublicKey, ReleaseManifest};

/// The production keys (current and next, one base64 `RW…` line each). Empty until the owner creates the signing
/// key (PROGRESS.md B1); until then stable and nightly updates are refused, never accepted unsigned.
const UPDATE_KEYS: &str = include_str!("../resources/update-keys.pub");

/// Trusted only by debug builds talking to a loopback test server; its secret is in `tests/support`.
#[cfg(debug_assertions)]
const TEST_KEY: &str = "RWTHHL5RYuPc3nVyZ0cOljgX0qZSCxHAYATThhnxx/MOXLNgDan0zp+s";

pub const MANIFEST: &str = link::MANIFEST_NAME;
pub const SIGNATURE: &str = link::SIGNATURE_NAME;

fn parse_keys(text: &str) -> Result<Vec<PublicKey>> {
    text.lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty() && !line.starts_with('#') && !line.starts_with("untrusted comment:")
        })
        .map(|line| {
            PublicKey::parse(line)
                .map_err(|e| Error::new("signature", format!("Built-in update key: {e}")))
        })
        .collect()
}

/// The keys a source trusts. `test` is true only for the debug-only loopback source.
pub fn trusted_keys(test: bool) -> Result<Vec<PublicKey>> {
    #[cfg_attr(not(debug_assertions), allow(unused_mut))]
    let mut keys = parse_keys(UPDATE_KEYS)?;
    #[cfg(debug_assertions)]
    if test {
        keys.extend(parse_keys(TEST_KEY)?);
    }
    #[cfg(not(debug_assertions))]
    let _ = test;
    Ok(keys)
}

fn link_channel(channel: Channel) -> link::Channel {
    match channel {
        Channel::Stable => link::Channel::Stable,
        Channel::Nightly => link::Channel::Nightly,
        Channel::Test => link::Channel::Test,
    }
}

/// Verifies a release's `manifest.json` (and, outside the test channel, its signature) for `id` on `channel`,
/// published under `tag`.
pub fn verify(
    id: &str,
    channel: Channel,
    tag: &str,
    manifest: &[u8],
    signature: Option<&str>,
    keys: &[PublicKey],
) -> Result<ReleaseManifest> {
    let parsed = match (channel, signature) {
        (Channel::Test, _) => ReleaseManifest::from_json(manifest)
            .map_err(|e| Error::new("manifest", format!("manifest.json: {e}")))?,
        (_, None) => {
            return Err(Error::new(
                "signature",
                "This release is not signed, so Tools won't install it.",
            ))
        }
        (_, Some(_)) if keys.is_empty() => {
            return Err(Error::new(
                "signature",
                "This build of Arcade Tools has no update signing key, so it can't verify updates. Install a newer \
                 Tools from its releases page.",
            ))
        }
        (_, Some(signature)) => link::verify_manifest(manifest, signature, keys).map_err(|e| {
            Error::new(
                "signature",
                format!("The release signature could not be verified ({e}). Nothing was downloaded."),
            )
        })?,
    };
    if parsed.app != id || parsed.channel != link_channel(channel) || parsed.tag != tag {
        return Err(Error::new(
            "signature",
            "The signed manifest belongs to a different app, channel or release.",
        ));
    }
    Ok(parsed)
}

/// The manifest entry for the chosen installer. `arcade-release.json` supplies the install arguments; the signed
/// manifest decides the bytes, so both must describe the same file and version.
pub fn entry_for(
    manifest: &ReleaseManifest,
    release: &Release,
    asset: &Asset,
) -> Result<FileEntry> {
    let entry = manifest.file(&asset.file).ok_or_else(|| {
        Error::new(
            "manifest",
            format!("{} is not listed in the signed manifest.", asset.file),
        )
    })?;
    if entry.sha256 != asset.sha256
        || asset.size.is_some_and(|size| size != entry.size)
        || !versions_match(&manifest.version, &release.version, release.channel)
    {
        return Err(Error::new(
            "manifest",
            "arcade-release.json does not match the signed manifest.",
        ));
    }
    Ok(entry.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(channel: link::Channel, tag: &str) -> ReleaseManifest {
        ReleaseManifest {
            schema: link::SCHEMA,
            app: "arcade.look".into(),
            version: "1.0.0".into(),
            tag: tag.into(),
            channel,
            chunking: Default::default(),
            files: vec![FileEntry::from_bytes("Look.AppImage".into(), b"look")],
        }
    }

    #[test]
    fn the_production_key_file_parses_and_the_test_key_is_debug_only() {
        let production = parse_keys(UPDATE_KEYS).unwrap();
        assert_eq!(trusted_keys(false).unwrap(), production);
        assert_eq!(trusted_keys(true).unwrap().len(), production.len() + 1);
    }

    #[test]
    fn stable_needs_a_trusted_signature_bound_to_app_channel_and_tag() {
        let key = link::SecretKey::generate().unwrap();
        let other = link::SecretKey::generate().unwrap();
        let keys = [key.public()];
        let m = manifest(link::Channel::Stable, "v1.0.0");
        let (json, sig) = link::sign_manifest(&m, &key);
        verify(
            "arcade.look",
            Channel::Stable,
            "v1.0.0",
            &json,
            Some(&sig),
            &keys,
        )
        .unwrap();
        let refused = |id, channel, tag, sig: Option<&str>, keys: &[PublicKey]| {
            verify(id, channel, tag, &json, sig, keys).unwrap_err().code
        };
        assert_eq!(
            refused("arcade.look", Channel::Stable, "v1.0.0", None, &keys),
            "signature"
        );
        assert_eq!(
            refused("arcade.look", Channel::Stable, "v1.0.0", Some(&sig), &[]),
            "signature"
        );
        assert_eq!(
            refused(
                "arcade.look",
                Channel::Stable,
                "v1.0.0",
                Some(&sig),
                &[other.public()]
            ),
            "signature"
        );
        assert_eq!(
            refused("arcade.lens", Channel::Stable, "v1.0.0", Some(&sig), &keys),
            "signature"
        );
        assert_eq!(
            refused("arcade.look", Channel::Nightly, "v1.0.0", Some(&sig), &keys),
            "signature"
        );
        assert_eq!(
            refused("arcade.look", Channel::Stable, "v1.0.1", Some(&sig), &keys),
            "signature"
        );
        let mut tampered = json.clone();
        let at = tampered.iter().position(|b| *b == b'1').unwrap();
        tampered[at] = b'2';
        assert!(verify(
            "arcade.look",
            Channel::Stable,
            "v1.0.0",
            &tampered,
            Some(&sig),
            &keys
        )
        .is_err());
    }

    #[test]
    fn rc_manifests_are_unsigned_but_still_bound_to_their_release() {
        let m = manifest(link::Channel::Test, "v1.0.0-rc.2");
        let json = m.to_json();
        verify(
            "arcade.look",
            Channel::Test,
            "v1.0.0-rc.2",
            &json,
            None,
            &[],
        )
        .unwrap();
        assert!(verify(
            "arcade.look",
            Channel::Test,
            "v1.0.0-rc.3",
            &json,
            None,
            &[]
        )
        .is_err());
        assert!(verify(
            "arcade.look",
            Channel::Stable,
            "v1.0.0-rc.2",
            &json,
            None,
            &[]
        )
        .is_err());
    }

    #[test]
    fn the_installer_must_be_the_file_the_manifest_lists() {
        let m = manifest(link::Channel::Stable, "v1.0.0");
        let entry = &m.files[0];
        let mut asset = Asset {
            os: crate::release::Os::Linux,
            arch: crate::release::Arch::X64,
            kind: crate::release::Kind::Appimage,
            file: entry.name.clone(),
            sha256: entry.sha256.clone(),
            size: Some(entry.size),
            silent: None,
            install_args: None,
        };
        let release = Release {
            schema: 1,
            id: "arcade.look".into(),
            version: "1.0.0".into(),
            channel: Channel::Stable,
            link_protocol: vec![1],
            notes: String::new(),
            assets: vec![asset.clone()],
        };
        assert_eq!(&entry_for(&m, &release, &asset).unwrap(), entry);
        asset.sha256 = "0".repeat(64);
        assert!(entry_for(&m, &release, &asset).is_err());
        asset.sha256 = entry.sha256.clone();
        asset.file = "Other.AppImage".into();
        assert!(entry_for(&m, &release, &asset).is_err());
        let mut newer = release.clone();
        newer.version = "1.0.1".into();
        asset.file = entry.name.clone();
        assert!(entry_for(&m, &newer, &asset).is_err());
    }
}
