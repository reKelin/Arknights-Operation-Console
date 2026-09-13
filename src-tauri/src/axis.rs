use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use specta::Type;

use crate::bindings::CommandError;

typify::import_types!("../protocol/axislink.schema.json");

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum DraftKind {
    Deploy,
    Skill,
    Retreat,
}

impl DraftKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Deploy => "deploy",
            Self::Skill => "skill",
            Self::Retreat => "retreat",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum DraftDirection {
    Up,
    Right,
    Down,
    Left,
}

impl DraftDirection {
    fn as_str(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Right => "right",
            Self::Down => "down",
            Self::Left => "left",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DraftTile {
    pub x: u16,
    pub y: u16,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DraftEvent {
    pub id: String,
    pub frame: u32,
    pub order: u32,
    pub kind: DraftKind,
    pub operator: Option<String>,
    pub tile: Option<DraftTile>,
    pub direction: Option<DraftDirection>,
    pub label: Option<String>,
    pub complete: bool,
}

impl DraftEvent {
    pub fn new(id: String, frame: u32, order: u32, kind: DraftKind) -> Self {
        let mut event = Self {
            id,
            frame,
            order,
            kind,
            operator: None,
            tile: None,
            direction: None,
            label: None,
            complete: false,
        };
        event.refresh_complete();
        event
    }

    pub fn refresh_complete(&mut self) {
        let operator_complete = self.operator.as_deref().is_some_and(valid_operator_id);
        self.complete = operator_complete
            && match self.kind {
                DraftKind::Deploy => self.tile.is_some() && self.direction.is_some(),
                DraftKind::Skill | DraftKind::Retreat => true,
            };
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DraftAxis {
    pub title: String,
    pub stage_id: Option<String>,
    pub events: Vec<DraftEvent>,
}

impl DraftAxis {
    pub fn demo() -> Self {
        let mut events = vec![
            complete_event("demo-001", 420, 0, DraftKind::Deploy, "部署 01"),
            complete_event("demo-002", 810, 1, DraftKind::Skill, "技能 01"),
            complete_event("demo-003", 1110, 2, DraftKind::Skill, "技能 02"),
            complete_event("demo-004", 1782, 3, DraftKind::Retreat, "撤退 01"),
            complete_event("demo-005", 2190, 4, DraftKind::Skill, "技能 03"),
            complete_event("demo-006", 2736, 5, DraftKind::Deploy, "部署 02"),
            complete_event("demo-007", 3150, 6, DraftKind::Retreat, "撤退 02"),
        ];
        for event in &mut events {
            event.refresh_complete();
        }
        Self {
            title: "交互 Demo 示例轴".to_string(),
            stage_id: Some("main_00-01".to_string()),
            events,
        }
    }

    pub fn sort_events(&mut self) {
        self.events.sort_by(|left, right| {
            left.frame
                .cmp(&right.frame)
                .then(left.order.cmp(&right.order))
                .then(left.id.cmp(&right.id))
        });
    }

    pub fn to_axis_json(&self) -> Result<Value, CommandError> {
        if self.title.trim().is_empty() {
            return Err(CommandError::field(
                "axis_incomplete",
                "轴标题不能为空",
                "title",
            ));
        }
        let stage_id = self
            .stage_id
            .as_deref()
            .filter(|value| valid_stage_id(value))
            .ok_or_else(|| {
                CommandError::field("axis_incomplete", "导出前必须填写合法的 stageId", "stageId")
            })?;

        let mut events = Vec::with_capacity(self.events.len());
        for event in &self.events {
            if !event.complete {
                return Err(CommandError::field(
                    "event_incomplete",
                    format!("操作点 {} 尚未补全执行参数", event.id),
                    format!("events.{}", event.id),
                ));
            }
            events.push(event_to_value(event)?);
        }

        let value = json!({
            "schemaVersion": 1,
            "title": self.title,
            "stageId": stage_id,
            "timebase": { "fps": 30 },
            "events": events,
        });
        validate_axis_value(&value)?;
        let _: AxisDocument = serde_json::from_value(value.clone()).map_err(|error| {
            CommandError::new(
                "schema_validation",
                format!("AxisLink Schema 校验失败：{error}"),
            )
        })?;
        Ok(value)
    }

    pub fn from_axis_json(value: Value) -> Result<Self, CommandError> {
        validate_axis_value(&value)?;
        let _: AxisDocument = serde_json::from_value(value.clone()).map_err(|error| {
            CommandError::new(
                "schema_validation",
                format!("AxisLink Schema 校验失败：{error}"),
            )
        })?;

        let object = value.as_object().expect("validated root object");
        let mut axis = Self {
            title: object["title"]
                .as_str()
                .expect("validated title")
                .to_string(),
            stage_id: Some(
                object["stageId"]
                    .as_str()
                    .expect("validated stageId")
                    .to_string(),
            ),
            events: object["events"]
                .as_array()
                .expect("validated events")
                .iter()
                .enumerate()
                .map(|(order, event)| event_from_value(event, order as u32))
                .collect::<Result<_, _>>()?,
        };
        axis.sort_events();
        Ok(axis)
    }
}

fn complete_event(id: &str, frame: u32, order: u32, kind: DraftKind, label: &str) -> DraftEvent {
    let deploy = matches!(kind, DraftKind::Deploy);
    DraftEvent {
        id: id.to_string(),
        frame,
        order,
        kind,
        operator: Some("char_002_amiya".to_string()),
        tile: deploy.then_some(DraftTile { x: 4, y: 2 }),
        direction: deploy.then_some(DraftDirection::Left),
        label: Some(label.to_string()),
        complete: true,
    }
}

fn event_to_value(event: &DraftEvent) -> Result<Value, CommandError> {
    let operator = event
        .operator
        .as_deref()
        .filter(|value| valid_operator_id(value))
        .ok_or_else(|| {
            CommandError::field(
                "event_incomplete",
                format!("操作点 {} 缺少合法干员 ID", event.id),
                format!("events.{}.operator", event.id),
            )
        })?;

    let mut value = json!({
        "id": event.id,
        "frame": event.frame,
        "kind": event.kind.as_str(),
        "operator": operator,
    });
    let object = value.as_object_mut().expect("event object");
    if let Some(label) = event.label.as_deref() {
        object.insert("label".to_string(), Value::String(label.to_string()));
    }
    if matches!(event.kind, DraftKind::Deploy) {
        let tile = event.tile.as_ref().ok_or_else(|| {
            CommandError::field(
                "event_incomplete",
                format!("部署点 {} 缺少格子", event.id),
                format!("events.{}.tile", event.id),
            )
        })?;
        let direction = event.direction.ok_or_else(|| {
            CommandError::field(
                "event_incomplete",
                format!("部署点 {} 缺少朝向", event.id),
                format!("events.{}.direction", event.id),
            )
        })?;
        object.insert("tile".to_string(), json!({ "x": tile.x, "y": tile.y }));
        object.insert(
            "direction".to_string(),
            Value::String(direction.as_str().to_string()),
        );
    }
    Ok(value)
}

fn event_from_value(value: &Value, order: u32) -> Result<DraftEvent, CommandError> {
    let object = value.as_object().expect("validated event");
    let kind = match object["kind"].as_str().expect("validated kind") {
        "deploy" => DraftKind::Deploy,
        "skill" => DraftKind::Skill,
        "retreat" => DraftKind::Retreat,
        _ => unreachable!("validated event kind"),
    };
    let tile = object.get("tile").map(|tile| {
        let tile = tile.as_object().expect("validated tile");
        DraftTile {
            x: tile["x"].as_u64().expect("validated tile x") as u16,
            y: tile["y"].as_u64().expect("validated tile y") as u16,
        }
    });
    let direction =
        object
            .get("direction")
            .and_then(Value::as_str)
            .map(|direction| match direction {
                "up" => DraftDirection::Up,
                "right" => DraftDirection::Right,
                "down" => DraftDirection::Down,
                "left" => DraftDirection::Left,
                _ => unreachable!("validated direction"),
            });
    let mut event = DraftEvent {
        id: object["id"].as_str().expect("validated id").to_string(),
        frame: object["frame"].as_u64().expect("validated frame") as u32,
        order,
        kind,
        operator: Some(
            object["operator"]
                .as_str()
                .expect("validated operator")
                .to_string(),
        ),
        tile,
        direction,
        label: object
            .get("label")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        complete: false,
    };
    event.refresh_complete();
    Ok(event)
}

fn validate_axis_value(value: &Value) -> Result<(), CommandError> {
    let object = value
        .as_object()
        .ok_or_else(|| CommandError::new("invalid_axis", "AxisLink 根节点必须是对象"))?;
    reject_unknown_fields(
        object,
        &["schemaVersion", "title", "stageId", "timebase", "events"],
        "",
    )?;
    if object.get("schemaVersion").and_then(Value::as_u64) != Some(1) {
        return Err(CommandError::field(
            "unsupported_version",
            "只支持 AxisLink schemaVersion 1",
            "schemaVersion",
        ));
    }
    let title = object
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if title.trim().is_empty() || title.chars().count() > 128 {
        return Err(CommandError::field(
            "invalid_title",
            "title 必须包含 1–128 个字符",
            "title",
        ));
    }
    let stage_id = object
        .get("stageId")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !valid_stage_id(stage_id) {
        return Err(CommandError::field(
            "invalid_stage_id",
            "stageId 格式无效",
            "stageId",
        ));
    }
    let timebase = object
        .get("timebase")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            CommandError::field("invalid_timebase", "timebase 必须是对象", "timebase")
        })?;
    reject_unknown_fields(timebase, &["fps"], "timebase")?;
    if timebase.get("fps").and_then(Value::as_u64) != Some(30) {
        return Err(CommandError::field(
            "invalid_timebase",
            "timebase.fps 必须为 30",
            "timebase.fps",
        ));
    }
    let events = object
        .get("events")
        .and_then(Value::as_array)
        .ok_or_else(|| CommandError::field("invalid_events", "events 必须是数组", "events"))?;
    let mut ids = HashSet::with_capacity(events.len());
    for (index, event) in events.iter().enumerate() {
        validate_event(event, index, &mut ids)?;
    }
    Ok(())
}

fn validate_event(
    value: &Value,
    index: usize,
    ids: &mut HashSet<String>,
) -> Result<(), CommandError> {
    let field = |name: &str| format!("events.{index}.{name}");
    let object = value
        .as_object()
        .ok_or_else(|| CommandError::field("invalid_event", "操作点必须是对象", field("")))?;
    let id = object.get("id").and_then(Value::as_str).unwrap_or_default();
    if !valid_event_id(id) {
        return Err(CommandError::field(
            "invalid_event_id",
            "操作点 ID 格式无效",
            field("id"),
        ));
    }
    if !ids.insert(id.to_string()) {
        return Err(CommandError::field(
            "duplicate_event_id",
            format!("操作点 ID {id} 重复"),
            field("id"),
        ));
    }
    let frame = object.get("frame").and_then(Value::as_u64);
    if frame.is_none() || frame > Some(i32::MAX as u64) {
        return Err(CommandError::field(
            "invalid_frame",
            "frame 必须是有效的非负整数",
            field("frame"),
        ));
    }
    let operator = object
        .get("operator")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !valid_operator_id(operator) {
        return Err(CommandError::field(
            "invalid_operator",
            "operator 必须使用 char_ 开头的角色键",
            field("operator"),
        ));
    }
    if object
        .get("label")
        .and_then(Value::as_str)
        .is_some_and(|label| label.chars().count() > 120)
    {
        return Err(CommandError::field(
            "invalid_label",
            "label 不能超过 120 个字符",
            field("label"),
        ));
    }
    match object.get("kind").and_then(Value::as_str) {
        Some("deploy") => {
            reject_unknown_fields(
                object,
                &[
                    "id",
                    "frame",
                    "kind",
                    "operator",
                    "tile",
                    "direction",
                    "label",
                ],
                &format!("events.{index}"),
            )?;
            let tile = object
                .get("tile")
                .and_then(Value::as_object)
                .ok_or_else(|| {
                    CommandError::field("invalid_tile", "部署操作必须包含 tile", field("tile"))
                })?;
            reject_unknown_fields(tile, &["x", "y"], &field("tile"))?;
            for axis in ["x", "y"] {
                let coordinate = tile.get(axis).and_then(Value::as_u64);
                if coordinate.is_none() || coordinate > Some(255) {
                    return Err(CommandError::field(
                        "invalid_tile",
                        "格子坐标必须是 0–255 的整数",
                        format!("{}.{}", field("tile"), axis),
                    ));
                }
            }
            if !matches!(
                object.get("direction").and_then(Value::as_str),
                Some("up" | "right" | "down" | "left")
            ) {
                return Err(CommandError::field(
                    "invalid_direction",
                    "部署朝向无效",
                    field("direction"),
                ));
            }
        }
        Some("skill" | "retreat") => {
            reject_unknown_fields(
                object,
                &["id", "frame", "kind", "operator", "label"],
                &format!("events.{index}"),
            )?;
        }
        _ => {
            return Err(CommandError::field(
                "invalid_kind",
                "kind 只能是 deploy、skill 或 retreat",
                field("kind"),
            ));
        }
    }
    Ok(())
}

fn reject_unknown_fields(
    object: &serde_json::Map<String, Value>,
    allowed: &[&str],
    parent: &str,
) -> Result<(), CommandError> {
    if let Some(field) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        let path = if parent.is_empty() {
            field.clone()
        } else {
            format!("{parent}.{field}")
        };
        return Err(CommandError::field(
            "unknown_field",
            format!("不支持字段 {path}"),
            path,
        ));
    }
    Ok(())
}

fn valid_event_id(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    value.len() <= 64
        && first.is_ascii_alphanumeric()
        && chars.all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | ':' | '-')
        })
}

fn valid_operator_id(value: &str) -> bool {
    value.len() <= 128
        && value.strip_prefix("char_").is_some_and(|rest| {
            !rest.is_empty()
                && rest
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '_')
        })
}

fn valid_stage_id(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    value.len() <= 128
        && first.is_ascii_alphanumeric()
        && chars.all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | '/' | '-')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_axis_round_trips() {
        let value: Value =
            serde_json::from_str(include_str!("../../examples/demo.axis.json")).unwrap();
        let axis = DraftAxis::from_axis_json(value).unwrap();
        let exported = axis.to_axis_json().unwrap();

        assert_eq!(exported["schemaVersion"], 1);
        assert_eq!(exported["events"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn incomplete_draft_cannot_be_exported() {
        let mut axis = DraftAxis::demo();
        axis.events.push(DraftEvent::new(
            "draft-test".to_string(),
            30,
            99,
            DraftKind::Deploy,
        ));

        let error = axis.to_axis_json().unwrap_err();

        assert_eq!(error.code, "event_incomplete");
    }

    #[test]
    fn duplicate_event_ids_are_rejected() {
        let mut value: Value =
            serde_json::from_str(include_str!("../../examples/demo.axis.json")).unwrap();
        let duplicate = value["events"][0].clone();
        value["events"].as_array_mut().unwrap().push(duplicate);

        let error = DraftAxis::from_axis_json(value).unwrap_err();

        assert_eq!(error.code, "duplicate_event_id");
    }
}
