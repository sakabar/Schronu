use crate::client::local_time::JAPAN_UTC_OFFSET_MINUTES;
use crate::client::work_sessions::{BrowserLocalStorage, KeyValueStorage};
use dioxus::prelude::*;
use js_sys::{Array, Date, Function, Intl::DateTimeFormat, Object, Reflect};
use std::cell::RefCell;
use std::collections::HashMap;
use wasm_bindgen::{JsCast, JsValue};

const STORAGE_KEY: &str = "schronu_web.display_timezone.v1";
const DEVICE: &str = "device";
const JAPAN: &str = "Asia/Tokyo";
const SETTINGS_ID: &str = "display-timezone-settings";
const FEATURED_ZONES: &[(&str, &str)] = &[
    ("America/Los_Angeles", "ロサンゼルス"),
    ("America/New_York", "ニューヨーク"),
    ("Europe/London", "ロンドン"),
    ("Europe/Paris", "パリ"),
    ("Asia/Singapore", "シンガポール"),
    ("UTC", "UTC"),
];

pub(super) struct DisplayTimezone {
    preference: String,
    zone: String,
    formatter: Option<DateTimeFormat>,
    offsets: RefCell<HashMap<i64, i32>>,
}

impl DisplayTimezone {
    pub(super) fn restore() -> Self {
        BrowserLocalStorage
            .get(STORAGE_KEY)
            .ok()
            .flatten()
            .and_then(|preference| Self::from_preference(&preference))
            .or_else(|| Self::from_preference(DEVICE))
            .unwrap_or_else(|| Self {
                preference: DEVICE.to_owned(),
                zone: JAPAN.to_owned(),
                formatter: None,
                offsets: RefCell::default(),
            })
    }

    fn from_preference(preference: &str) -> Option<Self> {
        let intl = Reflect::get(&js_sys::global(), &"Intl".into()).ok()?;
        let constructor: Function = Reflect::get(&intl, &"DateTimeFormat".into())
            .ok()?
            .dyn_into()
            .ok()?;
        let options = Object::new();
        Reflect::set(&options, &"hour".into(), &"numeric".into()).ok()?;
        Reflect::set(&options, &"timeZoneName".into(), &"longOffset".into()).ok()?;
        if preference != DEVICE {
            Reflect::set(&options, &"timeZone".into(), &preference.into()).ok()?;
        }
        let arguments = Array::of2(&"en-US".into(), &options);
        let formatter: DateTimeFormat = Reflect::construct(&constructor, &arguments)
            .ok()?
            .dyn_into()
            .ok()?;
        let zone = resolved_zone(&formatter)?;
        Some(Self {
            preference: preference.to_owned(),
            zone,
            formatter: Some(formatter),
            offsets: RefCell::default(),
        })
    }

    pub(super) fn offset_at(&self, epoch_ms: i64) -> Option<i32> {
        if self.is_japan() {
            return Some(JAPAN_UTC_OFFSET_MINUTES);
        }
        // The page also renders on timer ticks. Reuse the offset for unchanged instants.
        let mut offsets = self.offsets.borrow_mut();
        if let Some(offset) = offsets.get(&epoch_ms) {
            return Some(*offset);
        }
        let date = Date::new(&JsValue::from_f64(epoch_ms as f64));
        if !date.get_time().is_finite() {
            return None;
        }
        let offset = self
            .formatter
            .as_ref()?
            .format_to_parts(&date)
            .iter()
            .find_map(|part| {
                let kind = Reflect::get(&part, &"type".into()).ok()?.as_string()?;
                if kind != "timeZoneName" {
                    return None;
                }
                let value = Reflect::get(&part, &"value".into()).ok()?.as_string()?;
                parse_gmt_offset(&value)
            })?;
        if offsets.len() >= 4096 {
            offsets.clear();
        }
        offsets.insert(epoch_ms, offset);
        Some(offset)
    }

    fn is_japan(&self) -> bool {
        self.zone == JAPAN
    }

    pub(super) fn list_date_label(&self) -> Option<String> {
        (!self.is_japan())
            .then(|| format!("一覧の日付は日本時間 · 時刻は{}", zone_label(&self.zone)))
    }
}

fn resolved_zone(formatter: &DateTimeFormat) -> Option<String> {
    Reflect::get(&formatter.resolved_options(), &"timeZone".into())
        .ok()?
        .as_string()
}

fn parse_gmt_offset(value: &str) -> Option<i32> {
    let offset = value.strip_prefix("GMT")?;
    if offset.is_empty() {
        return Some(0);
    }
    let (hours, minutes) = offset.get(1..)?.split_once(':')?;
    let hours: i32 = hours.parse().ok()?;
    let minutes: i32 = minutes.split(':').next()?.parse().ok()?;
    let sign = match offset.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    Some(sign * (hours * 60 + minutes))
}

fn zone_label(zone: &str) -> &str {
    if zone == JAPAN {
        return "日本時間";
    }
    FEATURED_ZONES
        .iter()
        .find_map(|(id, label)| (*id == zone).then_some(*label))
        .unwrap_or(zone)
}

fn supported_zones() -> Option<Array> {
    let intl = Reflect::get(&js_sys::global(), &"Intl".into()).ok()?;
    let supported_values: Function = Reflect::get(&intl, &"supportedValuesOf".into())
        .ok()?
        .dyn_into()
        .ok()?;
    supported_values
        .call1(&intl, &"timeZone".into())
        .ok()?
        .dyn_into()
        .ok()
}

#[component]
pub(super) fn DisplayTimezoneSettings(mut timezone: Signal<DisplayTimezone>) -> Element {
    let mut choosing_city = use_signal(|| false);
    let mut query = use_signal(String::new);
    let mut error = use_signal(|| None::<&'static str>);
    let regions = use_hook(|| {
        supported_zones()
            .into_iter()
            .flat_map(|zones| {
                zones
                    .iter()
                    .filter_map(|value| value.as_string())
                    .collect::<Vec<_>>()
            })
            .filter(|zone| zone != JAPAN && !FEATURED_ZONES.iter().any(|(id, _)| *id == zone))
            .collect::<Vec<_>>()
    });
    let device = use_hook(|| {
        DisplayTimezone::from_preference(DEVICE)
            .map_or_else(|| JAPAN.to_owned(), |value| value.zone)
    });

    let select_zone = use_callback(move |preference: String| {
        let Some(value) = DisplayTimezone::from_preference(&preference) else {
            error.set(Some("このタイムゾーンは利用できません。"));
            return;
        };
        if BrowserLocalStorage.set(STORAGE_KEY, &preference).is_err() {
            error.set(Some("表示時刻を保存できませんでした。"));
            return;
        }
        timezone.set(value);
        error.set(None);
        choosing_city.set(false);
        query.set(String::new());
        close_settings();
    });

    let current = timezone.read();
    let uses_device = current.preference == DEVICE;
    let uses_japan = current.preference == JAPAN;
    let city_label = if uses_device || uses_japan {
        "タイムゾーンを選ぶ…".to_owned()
    } else {
        format!("タイムゾーンを選ぶ… ({})", zone_label(&current.zone))
    };

    let matching_regions: Vec<_> = if choosing_city() {
        let query = query().trim().to_lowercase().replace(' ', "_");
        FEATURED_ZONES
            .iter()
            .map(|(id, label)| (*id, *label))
            .chain(regions.iter().map(|id| (id.as_str(), id.as_str())))
            .filter(|(id, label)| {
                if query.is_empty() {
                    return FEATURED_ZONES.iter().any(|(featured, _)| featured == id);
                }
                id.to_lowercase().contains(&query) || label.to_lowercase().contains(&query)
            })
            .take(20)
            .collect()
    } else {
        Vec::new()
    };

    let no_matches = choosing_city() && matching_regions.is_empty();
    rsx! {
        details {
            id: SETTINGS_ID,
            class: "display-timezone-settings",
            onkeydown: move |event: KeyboardEvent| {
                if event.key() == Key::Escape {
                    event.prevent_default();
                    close_settings();
                }
            },
            summary { aria_label: "設定", title: "設定",
                span { aria_hidden: "true", "⚙" }
            }
            section { class: "display-timezone-menu", aria_labelledby: "display-timezone-heading",
                h2 { id: "display-timezone-heading", "表示時刻" }
                button {
                    r#type: "button",
                    aria_pressed: uses_device,
                    onclick: move |_| select_zone.call(DEVICE.to_owned()),
                    "端末に合わせる ({zone_label(&device)})"
                }
                button {
                    r#type: "button",
                    aria_pressed: uses_japan,
                    onclick: move |_| select_zone.call(JAPAN.to_owned()),
                    "日本時間"
                }
                button {
                    r#type: "button",
                    aria_expanded: choosing_city(),
                    onclick: move |_| {
                        let open = !choosing_city();
                        choosing_city.set(open);
                        query.set(String::new());
                    },
                    "{city_label}"
                }
                if choosing_city() {
                    input {
                        class: "task-name-filter-input",
                        r#type: "search",
                        aria_label: "都市名またはタイムゾーンを検索",
                        placeholder: "都市名またはタイムゾーン",
                        value: query(),
                        oninput: move |event| query.set(event.value()),
                        onmounted: move |event| async move {
                            let _ = event.data().set_focus(true).await;
                        },
                    }
                    div { class: "display-timezone-regions",
                        for (id, label) in matching_regions {
                            button {
                                key: "{id}",
                                r#type: "button",
                                aria_pressed: current.preference == id,
                                onclick: {
                                    let id = id.to_owned();
                                    move |_| select_zone.call(id.clone())
                                },
                                "{label}"
                            }
                        }
                        if no_matches {
                            p { role: "status", "該当するタイムゾーンがありません。" }
                        }
                    }
                }
                if let Some(error) = error() {
                    p { class: "display-timezone-error", role: "alert", "{error}" }
                }
            }
        }
    }
}

fn close_settings() {
    let Some(settings) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id(SETTINGS_ID))
    else {
        return;
    };
    let _ = settings.remove_attribute("open");
    if let Ok(Some(summary)) = settings.query_selector("summary") {
        if let Ok(summary) = summary.dyn_into::<web_sys::HtmlElement>() {
            let _ = summary.focus();
        }
    }
}
