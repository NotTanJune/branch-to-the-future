use std::fmt;

use serde::{de, Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    JavaScript,
    TypeScript,
    Python,
    Rust,
    Config,
    Route,
    Test,
    Schema,
    Worker,
    Service,
    Controller,
    UiComponent,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepoModel {
    pub repo_name: String,
    pub root_path: String,
    pub frameworks: Vec<String>,
    pub files: Vec<RepoFile>,
    pub routes: Vec<RouteInfo>,
    pub tests: Vec<String>,
    pub config_files: Vec<String>,
    pub risk_signals: Vec<RiskSignal>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepoFile {
    pub path: String,
    pub kind: FileKind,
    pub size: usize,
    pub symbols: Vec<String>,
    pub imports: Vec<String>,
    pub snippets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteInfo {
    pub path: String,
    pub method: String,
    pub route: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RiskSignal {
    pub path: String,
    pub signal: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParsedChangeRequest {
    pub change_type: String,
    pub domain: String,
    pub capabilities: Vec<String>,
    pub likely_layers: Vec<String>,
    pub keywords: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImpactAnalysis {
    pub summary: String,
    pub impact_path: Vec<ImpactFile>,
    pub risk_summary: Vec<String>,
    pub tests_to_add: Vec<String>,
    pub futures: Vec<ImplementationFuture>,
    pub recommended_future: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImpactFile {
    pub path: String,
    pub reason: String,
    #[serde(deserialize_with = "score")]
    pub impact_score: u8,
    #[serde(deserialize_with = "score")]
    pub confidence: u8,
    pub risk: RiskLevel,
    pub change_needed: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImplementationFuture {
    pub name: String,
    pub description: String,
    pub complexity: Complexity,
    pub risk: RiskLevel,
    pub affected_files: Vec<String>,
    #[serde(default)]
    pub benefits: Vec<String>,
    #[serde(default)]
    pub drawbacks: Vec<String>,
    #[serde(default)]
    pub patch_plan: Vec<String>,
    #[serde(default)]
    pub test_plan: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

impl fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RiskLevel::Low => write!(f, "low"),
            RiskLevel::Medium => write!(f, "medium"),
            RiskLevel::High => write!(f, "high"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Complexity {
    Low,
    Medium,
    High,
}

impl fmt::Display for Complexity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Complexity::Low => write!(f, "low"),
            Complexity::Medium => write!(f, "medium"),
            Complexity::High => write!(f, "high"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Input,
    RepoScan,
    ImpactExplorer,
    FileDetail,
    FuturesCompare,
    ArtifactGeneration,
    ExportSummary,
    Error,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimationStage {
    BootReveal,
    RepoMaterialize,
    ScanningSweep,
    StreamShimmer,
    ImpactTrace,
    RiskBloom,
    LockIn,
    ReplayTrace,
    ImpactToFutures,
    FuturesToImpact,
    DiagramReveal,
}

fn score<'de, D>(deserializer: D) -> Result<u8, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    let number = match value {
        serde_json::Value::Number(n) => n
            .as_u64()
            .or_else(|| n.as_i64().map(|v| v.max(0) as u64))
            .ok_or_else(|| de::Error::custom("score must be numeric"))?,
        serde_json::Value::String(s) => s
            .parse::<u64>()
            .map_err(|_| de::Error::custom("score string must be numeric"))?,
        _ => return Err(de::Error::custom("score must be number or numeric string")),
    };
    Ok(number.min(100) as u8)
}
