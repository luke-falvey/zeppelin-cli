//! Loading `~/.zeppelin.toml` and resolving which profile/credentials to use.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

/// The on-disk config file.
///
/// ```toml
/// default_profile = "dev"
/// username = "alice"        # default for every profile
/// password = "secret"
///
/// [profiles.dev]
/// base_url = "http://localhost:8080"
///
/// [profiles.prod]
/// base_url = "https://zeppelin.example.com"
/// username = "svc-zeppelin"  # overrides the top-level default
/// password = "other-secret"
/// ```
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FileConfig {
    pub default_profile: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    #[serde(default)]
    pub profiles: BTreeMap<String, Profile>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub base_url: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
}

/// Values given on the command line or through environment variables.
#[derive(Debug, Default)]
pub struct Overrides {
    pub profile: Option<String>,
    pub url: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
}

/// The final connection settings.
#[derive(Debug, Clone, Serialize)]
pub struct Resolved {
    pub profile: Option<String>,
    pub base_url: String,
    pub username: Option<String>,
    #[serde(skip)]
    pub password: Option<String>,
}

pub fn default_path() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow!("could not determine home directory"))?;
    Ok(home.join(".zeppelin.toml"))
}

/// Load the config file. A missing file is not an error (returns an empty config),
/// so the CLI can still be used with `--url` alone.
pub fn load(path: &Path) -> Result<FileConfig> {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            toml::from_str(&text).with_context(|| format!("invalid config file {}", path.display()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(FileConfig::default()),
        Err(e) => Err(e).with_context(|| format!("could not read {}", path.display())),
    }
}

impl FileConfig {
    /// Pick the profile name: explicit choice, then `default_profile`,
    /// then a profile literally named "default", then the only profile if there is one.
    pub fn profile_name(&self, explicit: Option<&str>) -> Result<Option<String>> {
        if let Some(name) = explicit {
            if !self.profiles.contains_key(name) {
                let known: Vec<_> = self.profiles.keys().cloned().collect();
                bail!(
                    "profile '{name}' not found in config (known profiles: {})",
                    if known.is_empty() {
                        "none".to_string()
                    } else {
                        known.join(", ")
                    }
                );
            }
            return Ok(Some(name.to_string()));
        }
        if let Some(name) = &self.default_profile {
            if !self.profiles.contains_key(name) {
                bail!("default_profile '{name}' is not defined under [profiles]");
            }
            return Ok(Some(name.clone()));
        }
        if self.profiles.contains_key("default") {
            return Ok(Some("default".to_string()));
        }
        if self.profiles.len() == 1 {
            return Ok(self.profiles.keys().next().cloned());
        }
        Ok(None)
    }

    pub fn resolve(&self, ov: &Overrides) -> Result<Resolved> {
        // With an explicit --url and no explicit profile, don't fail because no profile is chosen.
        let name = self.profile_name(ov.profile.as_deref())?;
        let profile = name.as_ref().and_then(|n| self.profiles.get(n));

        let base_url = ov
            .url
            .clone()
            .or_else(|| profile.and_then(|p| p.base_url.clone()))
            .ok_or_else(|| match &name {
                Some(n) => anyhow!("profile '{n}' has no base_url"),
                None if self.profiles.len() > 1 => anyhow!(
                    "several profiles are defined; choose one with --profile, ZEPPELIN_PROFILE or default_profile"
                ),
                None => anyhow!(
                    "no Zeppelin server configured: add a profile to ~/.zeppelin.toml or pass --url"
                ),
            })?;

        let username = ov
            .username
            .clone()
            .or_else(|| profile.and_then(|p| p.username.clone()))
            .or_else(|| self.username.clone());
        let password = ov
            .password
            .clone()
            .or_else(|| profile.and_then(|p| p.password.clone()))
            .or_else(|| self.password.clone());

        Ok(Resolved {
            profile: name,
            base_url,
            username,
            password,
        })
    }
}

/// Normalise a base URL into the `/api` root. Accepts `http://host:8080`,
/// `http://host:8080/`, `http://host:8080/zeppelin` or `http://host:8080/api`.
pub fn api_root(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.ends_with("/api") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/api")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(text: &str) -> FileConfig {
        toml::from_str(text).unwrap()
    }

    const SAMPLE: &str = r#"
        default_profile = "dev"
        username = "top"
        password = "toppw"

        [profiles.dev]
        base_url = "http://dev:8080"

        [profiles.prod]
        base_url = "https://prod/"
        username = "produser"
        password = "prodpw"
    "#;

    #[test]
    fn top_level_credentials_are_defaults() {
        let r = cfg(SAMPLE).resolve(&Overrides::default()).unwrap();
        assert_eq!(r.profile.as_deref(), Some("dev"));
        assert_eq!(r.base_url, "http://dev:8080");
        assert_eq!(r.username.as_deref(), Some("top"));
        assert_eq!(r.password.as_deref(), Some("toppw"));
    }

    #[test]
    fn profile_credentials_override_defaults() {
        let ov = Overrides {
            profile: Some("prod".into()),
            ..Default::default()
        };
        let r = cfg(SAMPLE).resolve(&ov).unwrap();
        assert_eq!(r.username.as_deref(), Some("produser"));
        assert_eq!(r.password.as_deref(), Some("prodpw"));
    }

    #[test]
    fn cli_overrides_win() {
        let ov = Overrides {
            profile: Some("prod".into()),
            url: Some("http://other".into()),
            username: Some("cli".into()),
            password: None,
        };
        let r = cfg(SAMPLE).resolve(&ov).unwrap();
        assert_eq!(r.base_url, "http://other");
        assert_eq!(r.username.as_deref(), Some("cli"));
        assert_eq!(r.password.as_deref(), Some("prodpw"));
    }

    #[test]
    fn unknown_profile_errors() {
        let ov = Overrides {
            profile: Some("nope".into()),
            ..Default::default()
        };
        let err = cfg(SAMPLE).resolve(&ov).unwrap_err().to_string();
        assert!(err.contains("nope") && err.contains("dev, prod"), "{err}");
    }

    #[test]
    fn single_profile_is_implicit() {
        let c = cfg("[profiles.only]\nbase_url = \"http://x\"");
        assert_eq!(
            c.resolve(&Overrides::default()).unwrap().base_url,
            "http://x"
        );
    }

    #[test]
    fn ambiguous_profiles_error() {
        let c = cfg("[profiles.a]\nbase_url = \"http://a\"\n[profiles.b]\nbase_url = \"http://b\"");
        assert!(c.resolve(&Overrides::default()).is_err());
    }

    #[test]
    fn unknown_keys_rejected() {
        assert!(toml::from_str::<FileConfig>("[profiles.a]\nbaseurl = \"x\"").is_err());
    }

    #[test]
    fn api_root_normalises() {
        assert_eq!(api_root("http://h:8080"), "http://h:8080/api");
        assert_eq!(api_root("http://h:8080/"), "http://h:8080/api");
        assert_eq!(api_root("http://h:8080/api/"), "http://h:8080/api");
        assert_eq!(api_root("https://h/zeppelin"), "https://h/zeppelin/api");
    }
}
