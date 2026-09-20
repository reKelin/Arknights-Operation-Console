use std::sync::Arc;

use crate::stage::{StageCatalog, StageRecognition};

pub struct OcrImage {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
}

pub struct StageOcrAccumulator {
    catalog: Arc<StageCatalog>,
    lines: Vec<String>,
}

impl StageOcrAccumulator {
    pub fn new(catalog: Arc<StageCatalog>) -> Self {
        Self {
            catalog,
            lines: Vec::new(),
        }
    }

    pub fn push(&mut self, text: &str) -> StageRecognition {
        for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
            if !self.lines.iter().any(|current| current == line) {
                self.lines.push(line.to_string());
            }
        }
        let raw_text = self.lines.join("\n");
        let matching_text = self
            .lines
            .iter()
            .filter(|line| !line.eq_ignore_ascii_case("OPERATION"))
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        let mut result = self.catalog.match_ocr(&matching_text);
        result.raw_text = raw_text;
        result
    }

    pub fn reset(&mut self) {
        self.lines.clear();
    }
}

pub fn crop_title(
    data: &[u8],
    width: u32,
    height: u32,
    row_pitch: u32,
) -> Result<OcrImage, String> {
    crop_region(data, width, height, row_pitch, [0, 0, 1920, 1080])
}

pub fn crop_region(
    data: &[u8],
    width: u32,
    height: u32,
    row_pitch: u32,
    region: [u32; 4],
) -> Result<OcrImage, String> {
    if width < 640
        || height < 360
        || row_pitch < width.saturating_mul(4)
        || data.len() < row_pitch as usize * height as usize
    {
        return Err("OCR 帧缓冲区尺寸无效".to_string());
    }
    let scale = (width as f64 / 1920.0).min(height as f64 / 1080.0);
    let offset_x = (width as f64 - 1920.0 * scale) / 2.0;
    let offset_y = (height as f64 - 1080.0 * scale) / 2.0;
    let reference =
        |value: u32, offset: f64| (offset + value as f64 * scale).round().max(0.0) as u32;
    let left = reference(region[0], offset_x).min(width);
    let right = reference(region[2], offset_x).min(width);
    let top = reference(region[1], offset_y).min(height);
    let bottom = reference(region[3], offset_y).min(height);
    if left >= right || top >= bottom {
        return Err("OCR 标题区域无效".to_string());
    }
    let crop_width = right - left;
    let crop_height = bottom - top;
    let mut pixels = Vec::with_capacity((crop_width * crop_height * 4) as usize);
    for y in top..bottom {
        let start = (y * row_pitch + left * 4) as usize;
        let end = start + (crop_width * 4) as usize;
        pixels.extend_from_slice(
            data.get(start..end)
                .ok_or_else(|| "OCR 标题区域超出帧缓冲区".to_string())?,
        );
    }
    Ok(OcrImage {
        pixels,
        width: crop_width,
        height: crop_height,
    })
}

#[cfg(windows)]
pub struct StageOcrRecognizer {
    _catalog: Arc<StageCatalog>,
    engine: windows::Media::Ocr::OcrEngine,
}

#[cfg(windows)]
impl StageOcrRecognizer {
    pub fn new(catalog: Arc<StageCatalog>) -> Result<Self, String> {
        use windows::{Globalization::Language, Media::Ocr::OcrEngine, core::HSTRING};

        let language = Language::CreateLanguage(&HSTRING::from("zh-CN"))
            .map_err(|error| format!("创建 zh-CN OCR 语言失败：{error}"))?;
        if !OcrEngine::IsLanguageSupported(&language).unwrap_or(false) {
            return Err(
                "Windows 未安装简体中文 OCR 语言包，请手动选择关卡或安装语言功能".to_string(),
            );
        }
        let engine = OcrEngine::TryCreateFromLanguage(&language)
            .map_err(|error| format!("创建 Windows OCR 引擎失败：{error}"))?;
        Ok(Self {
            _catalog: catalog,
            engine,
        })
    }

    pub fn recognize_text(&self, mut image: OcrImage) -> Result<String, String> {
        use windows::{
            Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap},
            Storage::Streams::DataWriter,
        };

        if image.height < 96 {
            let input = image::RgbaImage::from_raw(image.width, image.height, image.pixels)
                .ok_or("OCR 裁剪缓冲区无效")?;
            let output = image::imageops::resize(
                &input,
                image.width * 3,
                image.height * 3,
                image::imageops::FilterType::CatmullRom,
            );
            image = OcrImage {
                width: output.width(),
                height: output.height(),
                pixels: output.into_raw(),
            };
        }
        let max = windows::Media::Ocr::OcrEngine::MaxImageDimension().unwrap_or(2600);
        if image.width.max(image.height) > max {
            image = resize_bgra(image, max);
        }
        let width = i32::try_from(image.width).map_err(|_| "OCR 标题区域宽度过大".to_string())?;
        let height = i32::try_from(image.height).map_err(|_| "OCR 标题区域高度过大".to_string())?;
        let writer =
            DataWriter::new().map_err(|error| format!("创建 OCR 像素缓冲区失败：{error}"))?;
        writer
            .WriteBytes(&image.pixels)
            .map_err(|error| format!("写入 OCR 像素缓冲区失败：{error}"))?;
        let buffer = writer
            .DetachBuffer()
            .map_err(|error| format!("提交 OCR 像素缓冲区失败：{error}"))?;
        let bitmap =
            SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Bgra8, width, height)
                .map_err(|error| format!("创建 OCR 图像失败：{error}"))?;
        self.engine
            .RecognizeAsync(&bitmap)
            .and_then(|operation| operation.join())
            .and_then(|result| result.Text())
            .map(|text| text.to_string_lossy())
            .map_err(|error| format!("Windows OCR 识别失败（未打包运行时可手动选择关卡）：{error}"))
    }
}

fn resize_bgra(image: OcrImage, max_dimension: u32) -> OcrImage {
    let scale = f64::from(max_dimension) / f64::from(image.width.max(image.height));
    let width = (f64::from(image.width) * scale).round().max(1.0) as u32;
    let height = (f64::from(image.height) * scale).round().max(1.0) as u32;
    let mut pixels = vec![0; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let source_x = (u64::from(x) * u64::from(image.width) / u64::from(width)) as u32;
            let source_y = (u64::from(y) * u64::from(image.height) / u64::from(height)) as u32;
            let source = ((source_y * image.width + source_x) * 4) as usize;
            let target = ((y * width + x) * 4) as usize;
            pixels[target..target + 4].copy_from_slice(&image.pixels[source..source + 4]);
        }
    }
    OcrImage {
        pixels,
        width,
        height,
    }
}

#[cfg(not(windows))]
pub struct StageOcrRecognizer {
    _catalog: Arc<StageCatalog>,
}

#[cfg(not(windows))]
impl StageOcrRecognizer {
    pub fn new(catalog: Arc<StageCatalog>) -> Result<Self, String> {
        Ok(Self { _catalog: catalog })
    }

    pub fn recognize_text(&self, _image: OcrImage) -> Result<String, String> {
        Err("关卡 OCR 仅支持 Windows 10/11".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_crop_removes_source_stride_padding() {
        let width = 640;
        let height = 360;
        let row_pitch = width * 4 + 16;
        let frame = vec![255; (row_pitch * height) as usize];

        let crop = crop_title(&frame, width, height, row_pitch).unwrap();

        assert_eq!(crop.pixels.len(), (crop.width * crop.height * 4) as usize);
    }

    #[test]
    fn accumulator_keeps_later_code_and_name_after_operation_line() {
        let catalog = Arc::new(StageCatalog::embedded().unwrap());
        let mut accumulator = StageOcrAccumulator::new(catalog);

        assert!(accumulator.push("OPERATION").stage.is_none());
        assert!(accumulator.push("SR-EX-4").stage.is_none());
        let result = accumulator.push("反刍之堂");

        assert_eq!(
            result.stage.as_ref().map(|stage| stage.code.as_str()),
            Some("SR-EX-4")
        );
    }
}
