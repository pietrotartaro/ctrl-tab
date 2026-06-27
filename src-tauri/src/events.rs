//! Event payloads for the Rust → frontend switcher contract (see CLAUDE.md).
//!
//! The JSON field names are part of the contract, so the serialization is pure
//! and unit-tested.

use serde::Serialize;

/// One entry in the switcher.
#[derive(Serialize, Clone, Debug)]
pub struct SwitchItem {
    pub id: String,
    pub title: String,
    #[serde(rename = "appName")]
    pub app_name: String,
    #[serde(rename = "iconDataUrl")]
    pub icon_data_url: String,
}

/// `switcher:show` payload.
#[derive(Serialize, Clone, Debug)]
pub struct ShowPayload {
    pub mode: String,
    pub items: Vec<SwitchItem>,
    pub selected: usize,
}

/// `switcher:select` payload.
#[derive(Serialize, Clone, Debug)]
pub struct SelectPayload {
    pub selected: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_show() -> ShowPayload {
        ShowPayload {
            mode: "apps".into(),
            items: vec![SwitchItem {
                id: "810".into(),
                title: "Finder".into(),
                app_name: "Finder".into(),
                icon_data_url: "data:image/png;base64,AAA".into(),
            }],
            selected: 1,
        }
    }

    #[test]
    fn show_payload_uses_contract_field_names() {
        let v = serde_json::to_value(sample_show()).unwrap();
        assert_eq!(v["mode"], "apps");
        assert_eq!(v["selected"], 1);
        let item = &v["items"][0];
        assert_eq!(item["id"], "810");
        assert_eq!(item["title"], "Finder");
        // Contract requires camelCase for these two.
        assert_eq!(item["appName"], "Finder");
        assert_eq!(item["iconDataUrl"], "data:image/png;base64,AAA");
        assert!(item.get("app_name").is_none(), "must not emit snake_case app_name");
        assert!(
            item.get("icon_data_url").is_none(),
            "must not emit snake_case icon_data_url"
        );
    }

    #[test]
    fn select_payload_shape() {
        let v = serde_json::to_value(SelectPayload { selected: 3 }).unwrap();
        assert_eq!(v["selected"], 3);
    }
}
