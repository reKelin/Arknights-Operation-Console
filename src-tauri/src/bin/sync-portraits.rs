use image::{
    GrayImage,
    imageops::{FilterType, crop_imm, resize},
};
use serde::Deserialize;
use std::{fs, path::PathBuf, time::Duration};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Catalog {
    avatar_repository: String,
    avatar_revision: String,
    operators: Vec<Unit>,
}
#[derive(Deserialize)]
struct Unit {
    id: String,
    avatars: Vec<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data");
    let catalog: Catalog = serde_json::from_slice(&fs::read(data.join("operators.json"))?)?;
    let cache = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| data.join("../../.local/portrait-source"))
        .join(&catalog.avatar_revision);
    fs::create_dir_all(&cache)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()?;
    let mut entries = Vec::new();
    for unit in &catalog.operators {
        for path in &unit.avatars {
            entries.push((unit.id.clone(), path.clone()));
        }
    }
    let mut atlas = GrayImage::new(16, entries.len() as u32 * 16);
    for (index, (_, path)) in entries.iter().enumerate() {
        let file = cache.join(path.rsplit('/').next().unwrap());
        let bytes = match fs::read(&file) {
            Ok(bytes) => bytes,
            Err(_) => {
                let url = format!(
                    "https://cdn.jsdelivr.net/gh/{}@{}/{}",
                    catalog.avatar_repository,
                    catalog.avatar_revision,
                    path.replace('#', "%23").replace(' ', "%20")
                );
                let bytes = client
                    .get(url)
                    .send()?
                    .error_for_status()?
                    .bytes()?
                    .to_vec();
                fs::write(file, &bytes)?;
                bytes
            }
        };
        let image = image::load_from_memory(&bytes)?.to_luma8();
        let cropped = crop_imm(
            &image,
            image.width() / 6,
            image.height() / 4,
            image.width() * 2 / 3,
            image.height() / 2,
        )
        .to_image();
        let feature = resize(&cropped, 16, 16, FilterType::Triangle);
        image::imageops::replace(&mut atlas, &feature, 0, index as i64 * 16);
    }
    atlas.save(data.join("portrait-atlas.png"))?;
    fs::write(
        data.join("portrait-atlas.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "repository":catalog.avatar_repository,"revision":catalog.avatar_revision,"entries":entries
        }))?,
    )?;
    println!("已生成 {} 个头像特征", entries.len());
    Ok(())
}
