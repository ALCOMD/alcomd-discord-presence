//! Discord desktop presence implemented as a sandboxed extension guest.
#![forbid(unsafe_code)]

/// Returns the stable extension identifier.
#[must_use]
pub const fn extension_id() -> &'static str {
    "com.vrclearn.alcomd-discord-presence"
}

#[path = "protocol.rs"]
mod protocol;
#[path = "ui.rs"]
mod ui;

use crate::bindings;

use std::cell::RefCell;

use bindings::alcomd::extension::{host_data, host_projects, host_unity_sessions, types};
use bindings::exports::alcomd::extension::{guest_background, guest_lifecycle, guest_ui};
use serde::{Deserialize, Serialize};

const SETTINGS: &str = "presence-settings";

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct Settings {
    show_project: bool,
    show_version: bool,
    show_count: bool,
    custom_text: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct Activity {
    #[serde(rename = "type")]
    kind: u8,
    details: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    state: Option<String>,
}

#[derive(Default)]
struct Runtime {
    connection: protocol::Presence,
    observation: Option<host_unity_sessions::Observation>,
}

thread_local! {
    static RUNTIME: RefCell<Runtime> = RefCell::new(Runtime::default());
}

pub(super) struct Guest;

impl guest_lifecycle::Guest for Guest {
    fn activate(_: types::ActivationContext) -> Result<(), types::ExtensionError> {
        Ok(())
    }

    fn deactivate(_: types::StopReason) -> Result<(), types::ExtensionError> {
        RUNTIME.with(|runtime| {
            let mut runtime = runtime.borrow_mut();
            if runtime.connection.clear_and_close() {
                Ok(())
            } else {
                Err(types::ExtensionError {
                    code: types::ErrorCode::ResourceLimit,
                    diagnostic_id: None,
                })
            }
        })
    }
}

impl guest_background::Guest for Guest {
    fn tick() -> Result<(), types::ExtensionError> {
        let settings = load_settings().0;
        let observation = host_unity_sessions::current().ok();
        let activity = activity_for_observation(&settings, observation.as_ref());
        RUNTIME.with(|runtime| {
            let mut runtime = runtime.borrow_mut();
            runtime.connection.tick(activity.as_ref());
            runtime.observation = observation;
        });
        Ok(())
    }
}

impl guest_ui::Guest for Guest {
    fn open(_: String, locale: String) -> guest_ui::UiDocument {
        ui::open(locale)
    }
    fn refresh(_: String) -> guest_ui::UiDocument {
        ui::document(None)
    }
    fn dispatch(_: String, action: guest_ui::UiAction) -> guest_ui::UiDocument {
        ui::dispatch(action)
    }
    fn close(_: String) {}
}

fn load_settings() -> (Settings, Option<u64>) {
    if let Ok(Some(record)) = host_data::get(SETTINGS) {
        let settings = serde_json::from_slice::<Settings>(&record.bytes).unwrap_or_default();
        let custom_text = normalize_text(&settings.custom_text);
        (
            Settings {
                custom_text,
                ..settings
            },
            Some(record.key_revision),
        )
    } else {
        (Settings::default(), None)
    }
}

fn activity_for_observation(
    settings: &Settings,
    observation: Option<&host_unity_sessions::Observation>,
) -> Option<Activity> {
    let observation = observation?;
    let session = observation.current_session.as_ref()?;
    let summary = if settings.show_project || settings.show_version {
        host_projects::get_summary(&session.project_id).ok()
    } else {
        None
    };
    Some(activity(
        settings,
        summary.as_ref(),
        observation.editor_count,
    ))
}

fn activity(settings: &Settings, project: Option<&types::ProjectSummary>, count: u32) -> Activity {
    let details = if settings.custom_text.is_empty() {
        "Editing in Unity".to_owned()
    } else {
        settings.custom_text.clone()
    };
    let mut summary = Vec::new();
    if settings.show_project
        && let Some(project) = project
    {
        summary.push(normalize_text(&project.display_name));
    }
    if settings.show_version
        && let Some(version) = project.and_then(|project| project.unity_version.as_deref())
    {
        summary.push(format!("Unity {version}"));
    }
    if settings.show_count {
        summary.push(if count == 1 {
            "1 editor".to_owned()
        } else {
            format!("{count} editors")
        });
    }
    let state = normalize_text(&summary.join(" • "));
    Activity {
        kind: 0,
        details,
        state: (!state.is_empty()).then_some(state),
    }
}

fn normalize_text(text: &str) -> String {
    text.trim()
        .chars()
        .filter(|character| !character.is_control())
        .take(128)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn privacy_defaults_render_only_generic_unity_and_opt_ins_are_bounded() {
        let summary = types::ProjectSummary {
            project_id: "private-id".into(),
            display_name: "Synthetic project".into(),
            kind: types::ProjectKind::Vpm,
            unity_version: Some("2022.3.22f1".into()),
            revision: 1,
        };
        assert_eq!(
            activity(&Settings::default(), Some(&summary), 3).state,
            None
        );
        let settings = Settings {
            show_count: true,
            show_version: true,
            ..Settings::default()
        };
        let rendered = activity(&settings, Some(&summary), 3);
        assert_eq!(rendered.details, "Editing in Unity");
        assert_eq!(
            rendered.state.as_deref(),
            Some("Unity 2022.3.22f1 • 3 editors")
        );
        assert_eq!(normalize_text(&"界".repeat(130)).chars().count(), 128);
    }
}
