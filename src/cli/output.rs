use chrono::{DateTime, Utc};
use comfy_table::{Attribute, Cell, Color, ContentArrangement, Table, presets::UTF8_FULL};

use crate::Result;
use crate::tracking::{TrackingInfo, TrackingStatus};

#[derive(Debug, serde::Serialize)]
pub struct ListRow {
    pub id: String,
    pub provider: String,
    pub label: Option<String>,
    pub status: Option<String>,
    pub last_event_at: Option<DateTime<Utc>>,
    pub cached_at: Option<DateTime<Utc>>,
}

pub fn print_tracking(info: &TrackingInfo, as_json: bool) -> Result<()> {
    if as_json {
        let output = serde_json::to_string_pretty(info)?;
        println!("{output}");
        return Ok(());
    }

    print_tracking_pretty(info);
    Ok(())
}

fn print_tracking_pretty(info: &TrackingInfo) {
    println!("{}", render_tracking_pretty(info));
}

fn render_tracking_pretty(info: &TrackingInfo) -> String {
    let mut events = Table::new();
    events.load_preset(UTF8_FULL);
    events.set_content_arrangement(ContentArrangement::Dynamic);
    events.set_header([
        Cell::new("#")
            .add_attribute(Attribute::Bold)
            .fg(Color::Blue),
        Cell::new("TIME (UTC)")
            .add_attribute(Attribute::Bold)
            .fg(Color::Blue),
        Cell::new("EVENT")
            .add_attribute(Attribute::Bold)
            .fg(Color::Blue),
        Cell::new("CODE")
            .add_attribute(Attribute::Bold)
            .fg(Color::Blue),
    ]);

    const MAX_EVENTS: usize = 20;
    let timeline = info
        .events
        .iter()
        .rev()
        .take(MAX_EVENTS)
        .collect::<Vec<_>>();
    for (idx, event) in timeline.iter().enumerate() {
        let description_ansi = if idx + 1 == timeline.len() {
            status_ansi_color(&info.status)
        } else {
            "\x1b[37m"
        };
        let event_text = if let Some(location) = event.location.as_deref() {
            format!(
                "{description_ansi}{}\x1b[0m\n\x1b[32m{}\x1b[0m",
                event.description, location
            )
        } else {
            format!("{description_ansi}{}\x1b[0m", event.description)
        };
        let event_cell = Cell::new(event_text);
        let mut row_number = Cell::new((idx + 1).to_string()).fg(Color::DarkGrey);
        let mut time_cell =
            Cell::new(event.timestamp.format("%Y-%m-%d %H:%M").to_string()).fg(Color::Cyan);
        let code_cell = if let Some(raw_code) = event.raw_code.as_deref() {
            Cell::new(raw_code).fg(Color::DarkGrey)
        } else {
            Cell::new("-").fg(Color::DarkGrey)
        };

        if idx + 1 == timeline.len() {
            let highlight = status_color(&info.status);
            row_number = row_number.fg(highlight);
            time_cell = time_cell.fg(highlight);
        }

        events.add_row([row_number, time_cell, event_cell, code_cell]);
    }

    let mut summary = Table::new();
    summary.load_preset(UTF8_FULL);
    summary.set_content_arrangement(ContentArrangement::Dynamic);
    summary.set_header([
        Cell::new("FIELD")
            .add_attribute(Attribute::Bold)
            .fg(Color::Blue),
        Cell::new("VALUE")
            .add_attribute(Attribute::Bold)
            .fg(Color::Blue),
    ]);

    summary.add_row([
        Cell::new("Provider").fg(Color::DarkGrey),
        Cell::new(info.provider.as_str()).fg(Color::Cyan),
    ]);
    summary.add_row([
        Cell::new("Parcel ID").fg(Color::DarkGrey),
        Cell::new(info.parcel_id.as_str()).fg(Color::White),
    ]);
    summary.add_row([
        Cell::new("Status").fg(Color::DarkGrey),
        Cell::new(info.status.to_string()).fg(status_color(&info.status)),
    ]);

    if let Some(destination) = &info.destination_postcode {
        summary.add_row([
            Cell::new("Destination").fg(Color::DarkGrey),
            Cell::new(destination).fg(Color::Green),
        ]);
    }

    if let Some(estimated_delivery) = info.estimated_delivery {
        summary.add_row([
            Cell::new("Estimated delivery").fg(Color::DarkGrey),
            Cell::new(estimated_delivery.format("%Y-%m-%d %H:%M UTC").to_string()).fg(Color::Cyan),
        ]);
    }

    if let Some(event) = info.events.first() {
        summary.add_row([
            Cell::new("Last update").fg(Color::DarkGrey),
            Cell::new(event.timestamp.format("%Y-%m-%d %H:%M UTC").to_string()).fg(Color::Cyan),
        ]);
        summary.add_row([
            Cell::new("Last event").fg(Color::DarkGrey),
            Cell::new(event.description.as_str()).fg(status_color(&info.status)),
        ]);
    } else {
        summary.add_row([
            Cell::new("Last event").fg(Color::DarkGrey),
            Cell::new("(no events)").fg(Color::DarkGrey),
        ]);
    }

    let mut output = format!("{events}\n\n{summary}");
    if info.events.len() > MAX_EVENTS {
        output.push_str(&format!(
            "\n(showing {MAX_EVENTS} of {} total events)",
            info.events.len()
        ));
    }
    output
}

fn status_color(status: &TrackingStatus) -> Color {
    match status {
        TrackingStatus::Delivered => Color::Green,
        TrackingStatus::InTransit | TrackingStatus::OutForDelivery => Color::Yellow,
        TrackingStatus::Exception => Color::Red,
        TrackingStatus::PreShipment | TrackingStatus::Unknown => Color::DarkGrey,
    }
}

fn status_ansi_color(status: &TrackingStatus) -> &'static str {
    match status {
        TrackingStatus::Delivered => "\x1b[32m",
        TrackingStatus::InTransit | TrackingStatus::OutForDelivery => "\x1b[33m",
        TrackingStatus::Exception => "\x1b[31m",
        TrackingStatus::PreShipment | TrackingStatus::Unknown => "\x1b[90m",
    }
}

pub fn print_list(rows: &[ListRow], as_json: bool) -> Result<()> {
    if as_json {
        println!("{}", serde_json::to_string_pretty(rows)?);
        return Ok(());
    }

    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(["ID", "PROVIDER", "LABEL", "STATUS", "LAST EVENT"]);

    for row in rows {
        let status_text = row.status.as_deref().unwrap_or("(not fetched)");

        let color = match status_text {
            "Delivered" => status_color(&TrackingStatus::Delivered),
            "InTransit" => status_color(&TrackingStatus::InTransit),
            "OutForDelivery" => status_color(&TrackingStatus::OutForDelivery),
            "Exception" => status_color(&TrackingStatus::Exception),
            _ => status_color(&TrackingStatus::Unknown),
        };

        let last_event = row
            .last_event_at
            .map(|ts| ts.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "-".to_string());

        table.add_row([
            Cell::new(&row.id),
            Cell::new(&row.provider),
            Cell::new(row.label.clone().unwrap_or_else(|| "-".to_string())),
            Cell::new(status_text).fg(color),
            Cell::new(last_event),
        ]);
    }

    println!("{table}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tracking::{TrackingEvent, TrackingInfo};
    use chrono::TimeZone;

    #[test]
    fn render_tracking_pretty_includes_summary_and_events() {
        let info = TrackingInfo {
            parcel_id: "12345678".to_string(),
            provider: "mondial-relay".to_string(),
            status: TrackingStatus::Delivered,
            events: vec![
                TrackingEvent {
                    timestamp: Utc.with_ymd_and_hms(2026, 1, 8, 11, 14, 0).unwrap(),
                    description: "Colis livre au destinataire".to_string(),
                    location: Some("FR".to_string()),
                    raw_code: Some("DEL/01".to_string()),
                },
                TrackingEvent {
                    timestamp: Utc.with_ymd_and_hms(2026, 1, 7, 10, 42, 0).unwrap(),
                    description: "Colis en transit".to_string(),
                    location: Some("FR".to_string()),
                    raw_code: Some("TRN/02".to_string()),
                },
            ],
            estimated_delivery: Some(Utc.with_ymd_and_hms(2026, 1, 9, 0, 0, 0).unwrap()),
            destination_postcode: Some("00000".to_string()),
        };

        let rendered = render_tracking_pretty(&info);

        let events_pos = rendered.find("TIME (UTC)").unwrap();
        let summary_pos = rendered.find("FIELD").unwrap();
        assert!(events_pos < summary_pos);
        let transit_pos = rendered.find("Colis en transit").unwrap();
        let delivered_pos = rendered.find("Colis livre au destinataire").unwrap();
        assert!(transit_pos < delivered_pos);

        assert!(rendered.contains("FIELD"));
        assert!(rendered.contains("Provider"));
        assert!(rendered.contains("mondial-relay"));
        assert!(rendered.contains("Delivered"));
        assert!(rendered.contains("TIME (UTC)"));
        assert!(rendered.contains("Colis livre au destinataire"));
        assert!(rendered.contains("Colis en transit"));
        assert!(rendered.contains("FR"));
        assert!(rendered.contains("DEL/01"));
    }

    #[test]
    fn render_tracking_pretty_handles_empty_event_list() {
        let info = TrackingInfo {
            parcel_id: "AB12345678901".to_string(),
            provider: "laposte".to_string(),
            status: TrackingStatus::InTransit,
            events: Vec::new(),
            estimated_delivery: None,
            destination_postcode: None,
        };

        let rendered = render_tracking_pretty(&info);
        assert!(rendered.contains("(no events)"));
        assert!(rendered.contains("TIME (UTC)"));
    }
}

pub fn format_watch_line(
    now: DateTime<Utc>,
    provider: &str,
    parcel_id: &str,
    label: Option<&str>,
    status: &TrackingStatus,
    latest_event: Option<&str>,
) -> String {
    let label_text = label.map(|v| format!(" ({v})")).unwrap_or_default();
    let event_text = latest_event.unwrap_or("-");
    format!(
        "[{}] {provider}/{parcel_id}{label_text}: {:<11} - {event_text}",
        now.format("%Y-%m-%d %H:%M"),
        status
    )
}
