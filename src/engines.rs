//! Optional engine metadata. Detection is explicit worker work, never installation.
use crate::catalog::{check_schema, parse_version};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineCatalog {
    pub schema: u32,
    pub engines: Vec<Engine>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Engine {
    pub id: String,
    pub name: String,
    pub homepage: String,
    pub detect: Detection,
    pub used_by: Vec<EngineUse>,
    pub install: BTreeMap<String, String>,
    pub link_download: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Detection {
    pub binaries: Vec<String>,
    pub version_args: Vec<String>,
    #[serde(default)]
    pub min_version: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineUse {
    pub app: String,
    #[serde(rename = "for")]
    pub purpose: String,
}
impl EngineCatalog {
    /// Parse metadata in memory, safe on any thread; unknown fields are ignored.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        check_schema(bytes)?;
        let catalog: Self = serde_json::from_slice(bytes)?;
        let mut ids = HashSet::new();
        for engine in &catalog.engines {
            if engine.id.is_empty()
                || !ids.insert(&engine.id)
                || engine.detect.binaries.is_empty()
                || engine.detect.version_args.is_empty()
                || engine
                    .detect
                    .binaries
                    .iter()
                    .any(|b| b.is_empty() || b.contains(['/', '\\', ':']) || b == "." || b == "..")
            {
                return Err(Error::new("engines", "Invalid engine detection metadata."));
            }
            if let Some(version) = &engine.detect.min_version {
                parse_version(version)?;
            }
        }
        Ok(catalog)
    }
}
