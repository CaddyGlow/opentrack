use std::sync::LazyLock;

use regex::Regex;

/// Parsed Chronopost SOAP tracking response.
#[derive(Debug, Clone)]
pub struct ChronopostResponse {
    pub error_code: u32,
    pub skybill_number: Option<String>,
    pub events: Vec<ChronopostEvent>,
}

#[derive(Debug, Clone)]
pub struct ChronopostEvent {
    pub code: String,
    pub event_date: String,
    pub event_label: String,
    pub office_label: String,
    pub zip_code: String,
}

static ERROR_CODE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<errorCode>(\d+)</errorCode>").expect("valid regex"));

static SKYBILL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<skybillNumber>([^<]+)</skybillNumber>").expect("valid regex"));

static EVENT_BLOCK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<events>([\s\S]*?)</events>").expect("valid regex"));

static CODE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<code>([^<]*)</code>").expect("valid regex"));

static DATE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<eventDate>([^<]*)</eventDate>").expect("valid regex"));

static LABEL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<eventLabel>([^<]*)</eventLabel>").expect("valid regex"));

static OFFICE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<officeLabel>([^<]*)</officeLabel>").expect("valid regex"));

static ZIP_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<zipCode>([^<]*)</zipCode>").expect("valid regex"));

impl ChronopostResponse {
    /// Parse the SOAP XML body into a structured response.
    pub fn parse(xml: &str) -> crate::Result<Self> {
        let error_code = ERROR_CODE_RE
            .captures(xml)
            .and_then(|cap| cap[1].parse::<u32>().ok())
            .unwrap_or(0);

        let skybill_number = SKYBILL_RE
            .captures(xml)
            .map(|cap| cap[1].trim().to_string());

        let mut events = Vec::new();
        for block in EVENT_BLOCK_RE.captures_iter(xml) {
            let inner = &block[1];
            let code = extract(&CODE_RE, inner);
            let event_date = extract(&DATE_RE, inner);
            let event_label = extract(&LABEL_RE, inner);
            let office_label = extract(&OFFICE_RE, inner);
            let zip_code = extract(&ZIP_RE, inner);

            if !event_label.is_empty() {
                events.push(ChronopostEvent {
                    code,
                    event_date,
                    event_label,
                    office_label,
                    zip_code,
                });
            }
        }

        Ok(Self {
            error_code,
            skybill_number,
            events,
        })
    }
}

fn extract(re: &Regex, input: &str) -> String {
    re.captures(input)
        .map(|cap| cap[1].trim().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_intransit_fixture() {
        let xml = include_str!("../../../tests/fixtures/chronopost/tracking_intransit.xml");
        let parsed = ChronopostResponse::parse(xml).expect("valid xml");
        assert_eq!(parsed.error_code, 0);
        assert_eq!(parsed.skybill_number.as_deref(), Some("XM002774533TS"));
        assert!(!parsed.events.is_empty());
        assert_eq!(parsed.events[0].code, "DC");
    }

    #[test]
    fn parses_delivered_fixture() {
        let xml = include_str!("../../../tests/fixtures/chronopost/tracking_delivered.xml");
        let parsed = ChronopostResponse::parse(xml).expect("valid xml");
        assert_eq!(parsed.error_code, 0);
        let last = parsed.events.last().expect("has events");
        assert_eq!(last.code, "DI");
    }
}
