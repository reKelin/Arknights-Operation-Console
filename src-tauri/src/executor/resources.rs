use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use image::{DynamicImage, GrayImage};
use serde::Deserialize;

use crate::stage::{StageCatalog, StageCatalogEntry};

use super::geometry::{ProjectionMap, projection_file_name};

const OPERATORS_JSON: &str = include_str!("../../data/operators.json");
const PROJECTION_REPOSITORY: &str = "MaaAssistantArknights/MaaAssistantArknights";
const PROJECTION_REVISION: &str = "91cf0032d9cdbfd347870c6c9057e469ab3b6cb2";
const DOWNLOAD_LIMIT: usize = 8 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OperatorDocument {
    schema_version: u8,
    avatar_repository: String,
    avatar_revision: String,
    operators: Vec<OperatorEntry>,
}

#[derive(Clone, Deserialize)]
struct OperatorEntry {
    id: String,
    #[serde(rename = "name")]
    _name: String,
    avatars: Vec<String>,
}

pub struct ExecutionResources {
    operators: OperatorDocument,
    cache_root: PathBuf,
}

impl ExecutionResources {
    pub fn new(cache_root: PathBuf) -> Result<Self, String> {
        let operators: OperatorDocument = serde_json::from_str(OPERATORS_JSON)
            .map_err(|error| format!("解析干员目录失败：{error}"))?;
        if operators.schema_version != 1 {
            return Err("不支持的干员目录版本".to_string());
        }
        Ok(Self {
            operators,
            cache_root,
        })
    }

    pub fn load_avatars(&self, id: &str) -> Result<Vec<GrayImage>, String> {
        let operator = self
            .operator(id)
            .ok_or_else(|| format!("干员目录中没有 {id}"))?;
        if operator.avatars.is_empty() {
            return Err(format!("干员 {id} 没有可用头像"));
        }
        let mut images = Vec::new();
        for path in &operator.avatars {
            let file_name = Path::new(path)
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| "干员头像路径无效".to_string())?;
            let cache_path = self
                .cache_root
                .join("avatars")
                .join(&operator.id)
                .join(file_name);
            let bytes = read_or_download(
                &cache_path,
                &format!(
                    "https://cdn.jsdelivr.net/gh/{}@{}/{}",
                    self.operators.avatar_repository,
                    self.operators.avatar_revision,
                    encode_path(path)
                ),
            )?;
            let image = image::load_from_memory(&bytes)
                .map_err(|error| format!("解析干员头像失败：{error}"))?
                .to_luma8();
            images.push(image);
        }
        Ok(images)
    }

    pub fn load_projection(
        &self,
        catalog: &StageCatalog,
        stage_id: &str,
    ) -> Result<(StageCatalogEntry, ProjectionMap), String> {
        let stage = catalog
            .find(stage_id)
            .cloned()
            .ok_or_else(|| format!("关卡目录中没有 {stage_id}"))?;
        let file_name = projection_file_name(&stage);
        let cache_path = self.cache_root.join("projection").join(&file_name);
        let bytes = read_or_download(
            &cache_path,
            &format!(
                "https://cdn.jsdelivr.net/gh/{PROJECTION_REPOSITORY}@{PROJECTION_REVISION}/resource/Arknights-Tile-Pos/{}",
                encode_path(&file_name)
            ),
        )?;
        let map: ProjectionMap =
            serde_json::from_slice(&bytes).map_err(|error| format!("解析关卡投影失败：{error}"))?;
        if map.width == 0
            || map.height == 0
            || map.tiles.len() != map.height
            || map.tiles.iter().any(|row| row.len() != map.width)
        {
            return Err("关卡投影网格无效".to_string());
        }
        Ok((stage, map))
    }

    fn operator(&self, id: &str) -> Option<&OperatorEntry> {
        self.operators
            .operators
            .binary_search_by(|operator| operator.id.as_str().cmp(id))
            .ok()
            .map(|index| &self.operators.operators[index])
    }
}

fn encode_path(path: &str) -> String {
    path.replace('#', "%23").replace(' ', "%20")
}

fn read_or_download(path: &Path, url: &str) -> Result<Vec<u8>, String> {
    if let Ok(bytes) = fs::read(path)
        && !bytes.is_empty()
        && bytes.len() <= DOWNLOAD_LIMIT
    {
        return Ok(bytes);
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent("Arknights-Operation-Console")
        .build()
        .map_err(|error| format!("创建资源请求失败：{error}"))?;
    let response = client
        .get(url)
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|error| format!("下载执行资源失败：{error}"))?;
    if response
        .content_length()
        .is_some_and(|length| length > DOWNLOAD_LIMIT as u64)
    {
        return Err("执行资源文件过大".to_string());
    }
    let bytes = response
        .bytes()
        .map_err(|error| format!("读取执行资源失败：{error}"))?
        .to_vec();
    if bytes.is_empty() || bytes.len() > DOWNLOAD_LIMIT {
        return Err("执行资源文件大小无效".to_string());
    }
    let parent = path
        .parent()
        .ok_or_else(|| "执行资源缓存路径无效".to_string())?;
    fs::create_dir_all(parent).map_err(|error| format!("创建执行资源缓存失败：{error}"))?;
    fs::write(path, &bytes).map_err(|error| format!("保存执行资源缓存失败：{error}"))?;
    Ok(bytes)
}

pub fn resize_gray(image: &GrayImage, width: u32, height: u32) -> GrayImage {
    DynamicImage::ImageLuma8(image.clone())
        .resize_exact(width, height, image::imageops::FilterType::Triangle)
        .to_luma8()
}
