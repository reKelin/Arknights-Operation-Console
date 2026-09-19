use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use specta::Type;

const CATALOG_JSON: &str = include_str!("../data/stages.json");
const MAP_LIMIT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogDocument {
    schema_version: u8,
    source_repository: String,
    source_revision: String,
    stages: Vec<StageCatalogEntry>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StageCatalogEntry {
    pub id: String,
    pub code: String,
    pub name: String,
    pub level_path: String,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum StageMatchStatus {
    #[default]
    Unavailable,
    Matched,
    Ambiguous,
    Partial,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StageRecognition {
    pub status: StageMatchStatus,
    pub raw_text: String,
    pub stage: Option<StageCatalogEntry>,
    pub candidates: Vec<StageCatalogEntry>,
    pub warning: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StageMapBounds {
    pub width: u8,
    pub height: u8,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum StageIdentitySource {
    #[default]
    None,
    Ocr,
    Manual,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum StageSafetyStatus {
    #[default]
    Unverified,
    Matched,
    Mismatched,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StageSafetySnapshot {
    pub status: StageSafetyStatus,
    pub expected_stage_id: Option<String>,
    pub observed_stage: Option<StageCatalogEntry>,
    pub source: StageIdentitySource,
}

pub struct StageCatalog {
    source_repository: String,
    source_revision: String,
    stages: Vec<StageCatalogEntry>,
}

impl StageCatalog {
    pub fn embedded() -> Result<Self, String> {
        let document: CatalogDocument = serde_json::from_str(CATALOG_JSON)
            .map_err(|error| format!("解析内置关卡目录失败：{error}"))?;
        if document.schema_version != 1 {
            return Err(format!("不支持的关卡目录版本：{}", document.schema_version));
        }
        if document.stages.is_empty() {
            return Err("内置关卡目录为空".to_string());
        }
        for pair in document.stages.windows(2) {
            if pair[0].id >= pair[1].id {
                return Err("内置关卡目录 ID 未严格排序或存在重复".to_string());
            }
        }
        Ok(Self {
            source_repository: document.source_repository,
            source_revision: document.source_revision,
            stages: document.stages,
        })
    }

    pub fn source_revision(&self) -> &str {
        &self.source_revision
    }

    pub fn find(&self, id: &str) -> Option<&StageCatalogEntry> {
        self.stages
            .binary_search_by(|stage| stage.id.as_str().cmp(id))
            .ok()
            .map(|index| &self.stages[index])
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<StageCatalogEntry> {
        let query = query.trim();
        let normalized_code = normalize_code(query);
        let normalized_name = normalize_name(query);
        self.stages
            .iter()
            .filter(|stage| {
                query.is_empty()
                    || stage.id.eq_ignore_ascii_case(query)
                    || stage
                        .id
                        .to_ascii_lowercase()
                        .contains(&query.to_ascii_lowercase())
                    || (!normalized_code.is_empty()
                        && normalize_code(&stage.code).contains(&normalized_code))
                    || (!normalized_name.is_empty()
                        && normalize_name(&stage.name).contains(&normalized_name))
            })
            .take(limit)
            .cloned()
            .collect()
    }

    pub fn match_ocr(&self, raw_text: &str) -> StageRecognition {
        let raw_text = raw_text.trim().chars().take(512).collect::<String>();
        if raw_text.is_empty() {
            return StageRecognition {
                warning: Some("未识别到关卡代码或名称，请手动选择".to_string()),
                ..StageRecognition::default()
            };
        }
        let text_code = normalize_code(&raw_text);
        let text_name = normalize_name(&raw_text);
        let mut exact = Vec::new();
        let mut code_hits = Vec::new();
        let mut name_hits = Vec::new();
        for stage in &self.stages {
            let code = normalize_code(&stage.code);
            let name = normalize_name(&stage.name);
            let code_match = !code.is_empty() && text_code.contains(&code);
            let name_match = !name.is_empty() && text_name.contains(&name);
            if code_match {
                code_hits.push(stage.clone());
            }
            if name_match {
                name_hits.push(stage.clone());
            }
            if code_match && name_match {
                exact.push(stage.clone());
            }
        }
        if exact.len() == 1 {
            return StageRecognition {
                status: StageMatchStatus::Matched,
                raw_text,
                stage: exact.first().cloned(),
                candidates: exact,
                warning: None,
            };
        }
        if let Some(normal) = normal_and_challenge_pair(&exact) {
            return StageRecognition {
                status: StageMatchStatus::Matched,
                raw_text,
                stage: Some(normal),
                candidates: exact,
                warning: Some("标题无法区分普通与突袭，已默认普通关".to_string()),
            };
        }
        if !exact.is_empty() {
            exact.truncate(50);
            return StageRecognition {
                status: StageMatchStatus::Ambiguous,
                raw_text,
                stage: None,
                candidates: exact,
                warning: Some("关卡代码和名称对应多个地图，请手动选择".to_string()),
            };
        }
        let mut partial = if !code_hits.is_empty() {
            code_hits
        } else {
            name_hits
        };
        partial.truncate(50);
        StageRecognition {
            status: if partial.is_empty() {
                StageMatchStatus::Unavailable
            } else {
                StageMatchStatus::Partial
            },
            raw_text,
            stage: None,
            candidates: partial,
            warning: Some(if text_code.is_empty() && text_name.is_empty() {
                "未识别到关卡代码或名称，请手动选择".to_string()
            } else {
                "只识别到部分关卡信息，请手动确认".to_string()
            }),
        }
    }

    fn map_url(&self, stage: &StageCatalogEntry) -> Result<String, String> {
        if !safe_level_path(&stage.level_path) {
            return Err("关卡地图路径不安全".to_string());
        }
        Ok(format!(
            "https://cdn.jsdelivr.net/gh/{}@{}/zh_CN/gamedata/levels/{}",
            self.source_repository, self.source_revision, stage.level_path
        ))
    }
}

#[derive(Clone)]
pub struct StageRepository {
    catalog: Arc<StageCatalog>,
    cache_root: PathBuf,
}

impl StageRepository {
    pub fn new(catalog: Arc<StageCatalog>, cache_root: PathBuf) -> Self {
        Self {
            catalog,
            cache_root,
        }
    }

    pub fn catalog(&self) -> &StageCatalog {
        self.catalog.as_ref()
    }

    pub fn load_map(&self, stage_id: &str) -> Result<StageMapBounds, String> {
        let stage = self
            .catalog
            .find(stage_id)
            .ok_or_else(|| format!("关卡目录中没有 {stage_id}"))?;
        let cache_path = self
            .cache_root
            .join(self.catalog.source_revision())
            .join(&stage.level_path);
        if let Ok(bytes) = fs::read(&cache_path)
            && let Ok(bounds) = parse_map(&bytes)
        {
            return Ok(bounds);
        }
        let bytes = download_map(&self.catalog.map_url(stage)?)?;
        let bounds = parse_map(&bytes)?;
        write_cache(&cache_path, &bytes)?;
        Ok(bounds)
    }
}

fn normal_and_challenge_pair(entries: &[StageCatalogEntry]) -> Option<StageCatalogEntry> {
    if entries.len() != 2 {
        return None;
    }
    entries
        .iter()
        .find(|entry| !entry.id.ends_with("#f#"))
        .filter(|normal| {
            entries
                .iter()
                .any(|entry| entry.id == format!("{}#f#", normal.id))
        })
        .cloned()
}

fn normalize_code(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_uppercase)
        .collect()
}

fn normalize_name(value: &str) -> String {
    value
        .chars()
        .filter(|character| {
            character.is_alphanumeric()
                && !character.is_ascii_alphabetic()
                && !character.is_ascii_digit()
        })
        .collect()
}

fn safe_level_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains("..")
        && path.ends_with(".json")
        && path.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '/' | '_' | '-' | '.')
        })
}

fn download_map(url: &str) -> Result<Vec<u8>, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent("Arknights-Operation-Console")
        .build()
        .map_err(|error| format!("创建地图下载请求失败：{error}"))?;
    let response = client
        .get(url)
        .send()
        .map_err(|error| format!("下载关卡地图失败：{error}"))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err("上游数据中没有该关卡地图".to_string());
    }
    let response = response
        .error_for_status()
        .map_err(|error| format!("下载关卡地图失败：{error}"))?;
    if response
        .content_length()
        .is_some_and(|length| length > MAP_LIMIT_BYTES as u64)
    {
        return Err("关卡地图文件过大".to_string());
    }
    let bytes = response
        .bytes()
        .map_err(|error| format!("读取关卡地图失败：{error}"))?
        .to_vec();
    if bytes.len() > MAP_LIMIT_BYTES {
        return Err("关卡地图文件过大".to_string());
    }
    Ok(bytes)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LevelDocument {
    map_data: MapData,
}

#[derive(Deserialize)]
struct MapData {
    map: Vec<Vec<usize>>,
    tiles: Vec<serde_json::Value>,
}

fn parse_map(bytes: &[u8]) -> Result<StageMapBounds, String> {
    let level: LevelDocument =
        serde_json::from_slice(bytes).map_err(|error| format!("解析关卡地图失败：{error}"))?;
    let height = level.map_data.map.len();
    let width = level.map_data.map.first().map(Vec::len).unwrap_or_default();
    if !(1..=9).contains(&height)
        || !(1..=36).contains(&width)
        || level.map_data.map.iter().any(|row| row.len() != width)
    {
        return Err("关卡地图不是支持的 1–9 行、1–36 列矩形网格".to_string());
    }
    if level
        .map_data
        .map
        .iter()
        .flatten()
        .any(|index| *index >= level.map_data.tiles.len())
    {
        return Err("关卡地图引用了不存在的格子".to_string());
    }
    Ok(StageMapBounds {
        width: width as u8,
        height: height as u8,
    })
}

fn write_cache(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "地图缓存路径无效".to_string())?;
    fs::create_dir_all(parent).map_err(|error| format!("创建地图缓存目录失败：{error}"))?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, bytes).map_err(|error| format!("写入地图缓存失败：{error}"))?;
    if path.exists() {
        fs::remove_file(path).map_err(|error| format!("替换地图缓存失败：{error}"))?;
    }
    fs::rename(&temporary, path).map_err(|error| format!("保存地图缓存失败：{error}"))
}

pub fn validate_tile_in_map(tile: &str, bounds: StageMapBounds) -> Result<(), String> {
    tile_to_map_indices(tile, bounds).map(|_| ())
}

fn tile_to_map_indices(tile: &str, bounds: StageMapBounds) -> Result<(usize, usize), String> {
    let bytes = tile.as_bytes();
    if !(2..=3).contains(&bytes.len()) || !(b'A'..=b'I').contains(&bytes[0]) {
        return Err("格子短代码无效".to_string());
    }
    let row = bytes[0] - b'A';
    let column = tile[1..]
        .parse::<u8>()
        .ok()
        .and_then(|value| value.checked_sub(1))
        .ok_or_else(|| "格子短代码无效".to_string())?;
    if row >= bounds.height || column >= bounds.width {
        return Err(format!(
            "格子 {tile} 超出当前地图 {}×{} 的范围",
            bounds.width, bounds.height
        ));
    }
    Ok((usize::from(bounds.height - 1 - row), usize::from(column)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_rows_are_validated_without_assuming_dimensions() {
        let bounds =
            parse_map(br#"{"mapData":{"map":[[2,3],[0,1]],"tiles":[{},{},{},{}]}}"#).unwrap();

        assert_eq!(
            bounds,
            StageMapBounds {
                width: 2,
                height: 2
            }
        );
        assert!(validate_tile_in_map("A1", bounds).is_ok());
        assert!(validate_tile_in_map("B2", bounds).is_ok());
        assert!(validate_tile_in_map("C1", bounds).is_err());
    }

    #[test]
    fn short_codes_map_from_bottom_left_to_top_first_json_rows() {
        let bounds = StageMapBounds {
            width: 36,
            height: 9,
        };

        assert_eq!(tile_to_map_indices("A1", bounds).unwrap(), (8, 0));
        assert_eq!(tile_to_map_indices("I36", bounds).unwrap(), (0, 35));
    }

    #[test]
    fn ocr_requires_code_and_name_for_automatic_match() {
        let catalog = StageCatalog::embedded().unwrap();

        let matched = catalog.match_ocr("OPERATION\nSR-EX-8\n虚无之顶");
        let partial = catalog.match_ocr("OPERATION\nSR-EX-8");

        assert_eq!(matched.status, StageMatchStatus::Matched);
        assert_eq!(
            matched.stage.as_ref().map(|stage| stage.id.as_str()),
            Some("act54side_ex08")
        );
        assert_eq!(partial.status, StageMatchStatus::Partial);
        assert!(partial.stage.is_none());
    }
}
