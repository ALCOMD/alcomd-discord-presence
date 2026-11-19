use std::cell::Cell;

use super::{
    RUNTIME, SETTINGS, Settings, activity_for_observation,
    bindings::exports::alcomd::extension::guest_ui as ui, host_data, load_settings, normalize_text,
    protocol::Status,
};

const SAVE: &str = "save-settings";
const TITLE: &str = "ALCOMD Discord Presence";

thread_local! { static CHINESE: Cell<bool> = const { Cell::new(false) }; }

pub(super) fn open(locale: String) -> ui::UiDocument {
    CHINESE.set(locale.starts_with("zh"));
    document(None)
}

pub(super) fn dispatch(action: ui::UiAction) -> ui::UiDocument {
    let ui::UiAction::SubmitForm(form) = action else {
        return document(None);
    };
    if form.action_id != SAVE {
        return document(None);
    }
    let Some(settings) = submitted(form.values) else {
        return document(Some((t("Invalid settings", "设置无效"), ui::Tone::Danger)));
    };
    let saved = serde_json::to_vec(&settings)
        .ok()
        .is_some_and(|data| host_data::set(SETTINGS, &data, load_settings().1).is_ok());
    if saved {
        document(Some((t("Saved", "已保存"), ui::Tone::Success)))
    } else {
        document(Some((
            t("Could not save settings", "无法保存设置"),
            ui::Tone::Danger,
        )))
    }
}

fn submitted(values: Vec<ui::SubmittedField>) -> Option<Settings> {
    let mut result = Settings::default();
    let mut seen = std::collections::BTreeSet::new();
    for field in values {
        match (field.field_id.as_str(), field.value) {
            ("project", ui::FieldValue::Boolean(value)) => result.show_project = value,
            ("version", ui::FieldValue::Boolean(value)) => result.show_version = value,
            ("count", ui::FieldValue::Boolean(value)) => result.show_count = value,
            ("custom", ui::FieldValue::Text(value)) if value.chars().count() <= 128 => {
                result.custom_text = normalize_text(&value);
            }
            _ => return None,
        }
        if !seen.insert(field.field_id) {
            return None;
        }
    }
    (seen.len() == 4).then_some(result)
}

pub(super) fn document(feedback: Option<(&str, ui::Tone)>) -> ui::UiDocument {
    use ui::NodePayload;

    let settings = load_settings().0;
    let (connection, preview) = RUNTIME.with(|runtime| {
        let runtime = runtime.borrow();
        (
            runtime.connection.status(),
            activity_for_observation(&settings, runtime.observation.as_ref()),
        )
    });
    let (label, tone) = match connection {
        Status::Waiting => (t("Waiting for Discord", "等待 Discord"), ui::Tone::Neutral),
        Status::Connecting => (
            t("Connecting to Discord", "正在连接 Discord"),
            ui::Tone::Info,
        ),
        Status::Connected => (t("Connected", "已连接"), ui::Tone::Success),
        Status::Publishing => (t("Showing activity", "正在展示活动"), ui::Tone::Success),
        Status::ClientError => (
            t(
                "Discord connection failed. Retrying.",
                "Discord 连接失败，正在重试。",
            ),
            ui::Tone::Warning,
        ),
    };
    let mut nodes = vec![
        node(
            "root",
            None,
            0,
            NodePayload::Page(ui::PageNode {
                title: TITLE.into(),
            }),
        ),
        node(
            "connection",
            Some("root"),
            0,
            NodePayload::Status(ui::StatusNode {
                label: label.into(),
                tone,
            }),
        ),
        node(
            "preview",
            Some("root"),
            1,
            NodePayload::Section(ui::SectionNode {
                label: t("Preview", "预览").into(),
            }),
        ),
        node(
            "privacy",
            Some("root"),
            2,
            NodePayload::Form(ui::FormNode {
                submit_action_id: SAVE.into(),
                submit_label: t("Save", "保存").into(),
                disabled: false,
            }),
        ),
        toggle(
            "project",
            0,
            t("Show project name", "显示项目名"),
            settings.show_project,
        ),
        toggle(
            "version",
            1,
            t("Show Unity version", "显示 Unity 版本"),
            settings.show_version,
        ),
        toggle(
            "count",
            2,
            t("Show editor count", "显示编辑器数量"),
            settings.show_count,
        ),
        node(
            "custom",
            Some("privacy"),
            3,
            NodePayload::TextField(ui::TextFieldNode {
                field_id: "custom".into(),
                label: t("Custom text", "自定义文本").into(),
                initial_value: settings.custom_text,
                required: false,
                min_length: 0,
                max_length: 128,
                multiline: false,
                disabled: false,
                read_only: false,
                validation: None,
            }),
        ),
    ];
    if let Some(activity) = preview {
        nodes.push(node(
            "details",
            Some("preview"),
            0,
            NodePayload::Text(ui::TextNode {
                text: activity.details,
                tone: ui::Tone::Neutral,
            }),
        ));
        if let Some(text) = activity.state {
            nodes.push(node(
                "state",
                Some("preview"),
                1,
                NodePayload::Text(ui::TextNode {
                    text,
                    tone: ui::Tone::Neutral,
                }),
            ));
        }
    } else {
        nodes.push(node(
            "details",
            Some("preview"),
            0,
            NodePayload::Text(ui::TextNode {
                text: t("No Unity activity to display", "暂无可展示的 Unity 活动").into(),
                tone: ui::Tone::Neutral,
            }),
        ));
    }
    if let Some((label, tone)) = feedback {
        nodes.push(node(
            "feedback",
            Some("root"),
            3,
            NodePayload::Status(ui::StatusNode {
                label: label.into(),
                tone,
            }),
        ));
    }
    ui::UiDocument {
        title: TITLE.into(),
        nodes,
    }
}

fn t(english: &'static str, chinese: &'static str) -> &'static str {
    if CHINESE.get() { chinese } else { english }
}

fn node(id: &str, parent: Option<&str>, order: u32, payload: ui::NodePayload) -> ui::UiNode {
    ui::UiNode {
        node_id: id.into(),
        parent_id: parent.map(str::to_owned),
        order,
        payload,
    }
}

fn toggle(id: &str, order: u32, label: &str, initial: bool) -> ui::UiNode {
    node(
        id,
        Some("privacy"),
        order,
        ui::NodePayload::Switch(ui::SwitchNode {
            field_id: id.into(),
            label: label.into(),
            initial_value: initial,
            disabled: false,
            read_only: false,
            validation: None,
        }),
    )
}
