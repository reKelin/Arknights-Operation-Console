use serde::Deserialize;

use crate::{axis::DraftDirection, stage::StageCatalogEntry};

#[derive(Clone, Debug, Deserialize)]
pub struct ProjectionMap {
    pub width: usize,
    pub height: usize,
    pub tiles: Vec<Vec<ProjectionTile>>,
    pub view: [[f64; 3]; 2],
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionTile {
    pub height_type: i8,
}

pub fn projection_file_name(stage: &StageCatalogEntry) -> String {
    format!(
        "{}-{}",
        stage.id,
        stage.level_path.trim_end_matches(".json").replace('/', "-")
    )
    .to_lowercase()
        + ".json"
}

pub fn tile_indices(tile: &str, map: &ProjectionMap) -> Result<(usize, usize), String> {
    let bytes = tile.as_bytes();
    if !(2..=3).contains(&bytes.len()) || !(b'A'..=b'I').contains(&bytes[0]) {
        return Err("格子短代码无效".to_string());
    }
    let row_from_bottom = usize::from(bytes[0] - b'A');
    let column = tile[1..]
        .parse::<usize>()
        .ok()
        .and_then(|value| value.checked_sub(1))
        .ok_or_else(|| "格子短代码无效".to_string())?;
    if row_from_bottom >= map.height || column >= map.width {
        return Err(format!("格子 {tile} 超出关卡地图范围"));
    }
    Ok((map.height - 1 - row_from_bottom, column))
}

pub fn project_tile(tile: &str, map: &ProjectionMap, side: bool) -> Result<(f64, f64), String> {
    let (row, column) = tile_indices(tile, map)?;
    let tile_data = map
        .tiles
        .get(row)
        .and_then(|line| line.get(column))
        .ok_or_else(|| "关卡投影格子数据缺失".to_string())?;
    let [camera_x, camera_y, camera_z] = map.view[usize::from(side)];
    let mut point = [
        column as f64 - (map.width - 1) as f64 / 2.0,
        (map.height - 1) as f64 / 2.0 - row as f64,
        f64::from(tile_data.height_type) * -0.4,
        1.0,
    ];
    point[0] -= camera_x;
    point[1] -= camera_y;
    point[2] -= camera_z;
    if side {
        point = rotate_y(point, 10_f64.to_radians());
    }
    point = rotate_x(point, 30_f64.to_radians());
    let tan = 20_f64.to_radians().tan();
    let clip_x = (9.0 / 16.0) * point[0] / tan;
    let clip_y = point[1] / tan;
    let clip_w = -point[2];
    if clip_w.abs() < f64::EPSILON {
        return Err("关卡投影落在相机平面".to_string());
    }
    Ok((
        ((clip_x / clip_w) + 1.0) / 2.0,
        1.0 - ((clip_y / clip_w) + 1.0) / 2.0,
    ))
}

fn rotate_x(point: [f64; 4], angle: f64) -> [f64; 4] {
    [
        point[0],
        point[1] * angle.cos() - point[2] * angle.sin(),
        -point[1] * angle.sin() - point[2] * angle.cos(),
        point[3],
    ]
}

fn rotate_y(point: [f64; 4], angle: f64) -> [f64; 4] {
    [
        point[0] * angle.cos() + point[2] * angle.sin(),
        point[1],
        -point[0] * angle.sin() + point[2] * angle.cos(),
        point[3],
    ]
}

pub fn direction_target(
    origin: (i32, i32),
    direction: DraftDirection,
    distance: i32,
) -> (i32, i32) {
    match direction {
        DraftDirection::None => origin,
        DraftDirection::Up => (origin.0, origin.1 - distance),
        DraftDirection::Right => (origin.0 + distance, origin.1),
        DraftDirection::Down => (origin.0, origin.1 + distance),
        DraftDirection::Left => (origin.0 - distance, origin.1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> ProjectionMap {
        ProjectionMap {
            width: 36,
            height: 9,
            tiles: vec![vec![ProjectionTile { height_type: 0 }; 36]; 9],
            view: [[0.0, -4.81, -7.76], [0.6, -5.31, -8.64]],
        }
    }

    #[test]
    fn runner_short_codes_keep_letters_as_bottom_up_rows() {
        assert_eq!(tile_indices("A1", &map()).unwrap(), (8, 0));
        assert_eq!(tile_indices("I36", &map()).unwrap(), (0, 35));
    }

    #[test]
    fn projection_is_finite_and_inside_normalized_view() {
        let point = project_tile("E18", &map(), false).unwrap();
        assert!(point.0.is_finite() && point.1.is_finite());
        assert!((0.0..=1.0).contains(&point.0));
        assert!((0.0..=1.0).contains(&point.1));
    }
}
