//! Weather panel patterned after xfce4-weather-plugin
//! (`<https://gitlab.xfce.org/panel-plugins/xfce4-weather-plugin>`).
//!
//! The XFCE plugin fetches data from
//! `<https://api.met.no/weatherapi/locationforecast/2.0/complete?lat=…&lon=…>`,
//! the Norwegian Met Institute's free-tier locationforecast endpoint.
//! We follow the same contract:
//!
//! - GET the locationforecast JSON with a descriptive `User-Agent`
//!   header (met.no rate-limits anonymous traffic).
//! - Parse `properties.timeseries[0]` for *now*, taking
//!   `instant.details.air_temperature` and
//!   `next_1_hours.summary.symbol_code`.
//! - Render the result in a `gtk::Popover` anchored to the top-bar
//!   clock.
//!
//! HTTP via the system `curl` keeps this module dependency-free; pulling
//! `ureq` / `reqwest` into the workspace for one GET would balloon the
//! build graph. Failure modes (network down, met.no 5xx, malformed
//! JSON) degrade to a "Weather unavailable" label rather than crashing
//! the panel.

use std::process::Command;

use gtk::prelude::*;

/// xfce4-weather-plugin reads the same endpoint. Path + query convention
/// matches the public API contract documented at
/// <https://api.met.no/weatherapi/locationforecast/2.0/documentation>.
const MET_NO_ENDPOINT: &str = "https://api.met.no/weatherapi/locationforecast/2.0/complete";

/// met.no requires a descriptive User-Agent including a contact address;
/// anonymous / generic UAs are throttled or 403'd. xfce4-weather-plugin
/// sends `xfce4-weather-plugin/<version> <contact>`; we follow the same
/// pattern with the project URL.
const USER_AGENT: &str = "mackes-panel/1.0 (+https://github.com/matthewmackes/MAP2-RELEASES)";

/// curl deadline. Above the met.no p99 latency budget; below the user-
/// perceptible "is the popover broken" threshold.
const FETCH_TIMEOUT_SECS: &str = "6";

/// Parsed slice of the met.no response — enough to render the popover
/// without retaining the full forecast in memory.
#[derive(Debug, Clone, Default)]
pub struct Conditions {
    pub temp_c: Option<f64>,
    pub symbol_code: Option<String>,
}

/// Fetch *current* conditions for `(lat, lon)` synchronously via curl.
/// Returns `None` on any failure — caller renders a fallback message.
pub fn fetch(lat: f64, lon: f64) -> Option<Conditions> {
    let url = format!("{MET_NO_ENDPOINT}?lat={lat}&lon={lon}");
    let out = Command::new("curl")
        .args([
            "-sfL",
            "--max-time",
            FETCH_TIMEOUT_SECS,
            "-A",
            USER_AGENT,
            &url,
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_response(&out.stdout)
}

fn parse_response(body: &[u8]) -> Option<Conditions> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    let entry = v
        .get("properties")?
        .get("timeseries")?
        .as_array()?
        .first()?;
    let data = entry.get("data")?;
    let temp_c = data
        .get("instant")?
        .get("details")?
        .get("air_temperature")
        .and_then(serde_json::Value::as_f64);
    let symbol_code = data
        .get("next_1_hours")
        .and_then(|h| h.get("summary"))
        .and_then(|s| s.get("symbol_code"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    Some(Conditions {
        temp_c,
        symbol_code,
    })
}

/// Build a `gtk::Popover` anchored to `anchor`. Shows a "Loading…"
/// placeholder, kicks off the curl fetch on the next idle tick, and
/// updates the labels in place when the response lands.
#[must_use]
pub fn build_popover(anchor: &gtk::Widget, lat: f64, lon: f64) -> gtk::Popover {
    let popover = gtk::Popover::new(Some(anchor));
    popover.set_widget_name("mackes-weather-popover");
    popover.set_position(gtk::PositionType::Bottom);
    popover.set_constrain_to(gtk::PopoverConstraint::None);

    let column = gtk::Box::new(gtk::Orientation::Vertical, 8);
    column.set_widget_name("mackes-weather-column");
    column.set_margin_start(16);
    column.set_margin_end(16);
    column.set_margin_top(12);
    column.set_margin_bottom(12);

    let title = gtk::Label::new(Some("Weather"));
    title.set_widget_name("mackes-weather-title");
    title.set_halign(gtk::Align::Start);
    column.pack_start(&title, false, false, 0);

    let temp_label = gtk::Label::new(Some("…"));
    temp_label.set_widget_name("mackes-weather-temp");
    temp_label.set_halign(gtk::Align::Start);
    column.pack_start(&temp_label, false, false, 0);

    let symbol_label = gtk::Label::new(Some("Loading…"));
    symbol_label.set_widget_name("mackes-weather-symbol");
    symbol_label.set_halign(gtk::Align::Start);
    column.pack_start(&symbol_label, false, false, 0);

    let footer = gtk::Label::new(Some(&format!("{lat:.3}, {lon:.3} · api.met.no")));
    footer.set_widget_name("mackes-weather-footer");
    footer.set_halign(gtk::Align::Start);
    column.pack_start(&footer, false, false, 0);

    popover.add(&column);

    // glib::idle_add_local_once defers the blocking curl call until the
    // popover has actually rendered, so the user sees the "Loading…"
    // frame before we stall on the network round-trip.
    glib::idle_add_local_once(move || {
        if let Some(c) = fetch(lat, lon) {
            let temp = c
                .temp_c
                .map_or_else(|| "—".to_owned(), |t| format!("{t:.0} °C"));
            temp_label.set_text(&temp);
            symbol_label.set_text(c.symbol_code.as_deref().unwrap_or("clear-day"));
        } else {
            temp_label.set_text("—");
            symbol_label.set_text("Weather unavailable");
        }
    });

    popover
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_met_no_locationforecast_shape() {
        let body = br#"{
            "type": "Feature",
            "properties": {
                "timeseries": [{
                    "time": "2026-05-18T19:00:00Z",
                    "data": {
                        "instant": { "details": { "air_temperature": 14.2 } },
                        "next_1_hours": {
                            "summary": { "symbol_code": "partlycloudy_day" }
                        }
                    }
                }]
            }
        }"#;
        let c = parse_response(body).expect("parse");
        assert_eq!(c.temp_c, Some(14.2));
        assert_eq!(c.symbol_code.as_deref(), Some("partlycloudy_day"));
    }

    #[test]
    fn missing_fields_degrade_to_none() {
        let body = br#"{"properties": {"timeseries": [{"data": {"instant": {"details": {}}}}]}}"#;
        let c = parse_response(body).expect("parse");
        assert_eq!(c.temp_c, None);
        assert_eq!(c.symbol_code, None);
    }

    #[test]
    fn empty_timeseries_returns_none() {
        let body = br#"{"properties": {"timeseries": []}}"#;
        assert!(parse_response(body).is_none());
    }

    #[test]
    fn build_popover_has_four_labels_in_a_column() {
        let _g = crate::test_env::env_lock();
        if !crate::test_env::try_init_gtk_serialized() {
            return;
        }
        let anchor = gtk::Label::new(Some("anchor"));
        let popover = build_popover(anchor.upcast_ref(), 59.91, 10.75);
        assert_eq!(popover.widget_name(), "mackes-weather-popover");
        // The popover's child is a Box with title/temp/symbol/footer
        // labels — 4 children total.
        let column = popover
            .child()
            .expect("popover must have a child")
            .downcast::<gtk::Box>()
            .expect("popover child must be a Box");
        assert_eq!(column.widget_name(), "mackes-weather-column");
        assert_eq!(
            column.children().len(),
            4,
            "expected 4 labels: title + temp + symbol + footer"
        );
    }

    #[test]
    fn build_popover_footer_carries_coordinates_and_attribution() {
        let _g = crate::test_env::env_lock();
        if !crate::test_env::try_init_gtk_serialized() {
            return;
        }
        let anchor = gtk::Label::new(Some("anchor"));
        let popover = build_popover(anchor.upcast_ref(), 47.123, -122.456);
        let column = popover
            .child()
            .unwrap()
            .downcast::<gtk::Box>()
            .unwrap();
        let footer = column
            .children()
            .into_iter()
            .find(|w| w.widget_name() == "mackes-weather-footer")
            .expect("footer label must be present")
            .downcast::<gtk::Label>()
            .unwrap();
        let text = footer.text().to_string();
        assert!(text.contains("47.123"), "footer must show latitude: {text}");
        assert!(text.contains("-122.456"), "footer must show longitude: {text}");
        assert!(
            text.contains("met.no"),
            "footer must attribute the data source: {text}"
        );
    }
}
