use serde::Deserialize;
use serde::de::Error as _;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct MondialRelayResponse {
    #[serde(default, alias = "CodeRetour", alias = "codeRetour")]
    pub code_retour: Option<u32>,
    #[serde(default, alias = "Message", alias = "message")]
    pub message: Option<String>,

    #[serde(
        default,
        alias = "SuiviParEtapes",
        alias = "suiviParEtapes",
        deserialize_with = "deserialize_vec_or_map"
    )]
    pub suivi_par_etapes: Vec<MondialRelayStep>,
    #[serde(
        default,
        alias = "Evenements",
        alias = "evenements",
        deserialize_with = "deserialize_vec_or_map"
    )]
    pub evenements: Vec<MondialRelayEvent>,

    #[serde(
        default,
        alias = "CodePostal",
        alias = "codePostal",
        alias = "CodePostalDestinataire"
    )]
    pub destination_postcode: Option<String>,

    #[serde(
        default,
        alias = "DateLivraisonPrevue",
        alias = "dateLivraisonPrevue",
        alias = "DatePrevisionnelle",
        alias = "datePrevisionnelle",
        alias = "EstimatedDeliveryDate"
    )]
    pub estimated_delivery: Option<String>,

    #[serde(default, alias = "Data", alias = "data")]
    pub data: Option<Box<MondialRelayResponse>>,
    #[serde(default, alias = "Expedition", alias = "expedition")]
    pub expedition: Option<Box<MondialRelayResponse>>,
    #[serde(default, alias = "Result", alias = "result")]
    pub result: Option<Box<MondialRelayResponse>>,
}

impl MondialRelayResponse {
    fn nested(&self) -> Option<&MondialRelayResponse> {
        self.data
            .as_deref()
            .or(self.expedition.as_deref())
            .or(self.result.as_deref())
    }

    pub fn code_retour_recursive(&self) -> Option<u32> {
        self.code_retour
            .or_else(|| self.nested().and_then(Self::code_retour_recursive))
    }

    pub fn message_recursive(&self) -> Option<&str> {
        self.message
            .as_deref()
            .or_else(|| self.nested().and_then(Self::message_recursive))
    }

    pub fn steps_recursive(&self) -> &[MondialRelayStep] {
        if !self.suivi_par_etapes.is_empty() {
            &self.suivi_par_etapes
        } else {
            self.nested().map_or(&[], Self::steps_recursive)
        }
    }

    pub fn events_recursive(&self) -> &[MondialRelayEvent] {
        if !self.evenements.is_empty() {
            &self.evenements
        } else {
            self.nested().map_or(&[], Self::events_recursive)
        }
    }

    pub fn destination_postcode_recursive(&self) -> Option<&str> {
        self.destination_postcode
            .as_deref()
            .or_else(|| self.nested().and_then(Self::destination_postcode_recursive))
    }

    pub fn estimated_delivery_recursive(&self) -> Option<&str> {
        self.estimated_delivery
            .as_deref()
            .or_else(|| self.nested().and_then(Self::estimated_delivery_recursive))
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct MondialRelayStep {
    #[serde(
        default,
        alias = "Libelle",
        alias = "libelle",
        alias = "Label",
        alias = "label",
        alias = "Intitule",
        alias = "intitule"
    )]
    pub label: Option<String>,
    #[serde(default, alias = "Code", alias = "code")]
    pub code: Option<String>,
    #[serde(
        default,
        alias = "Etape",
        alias = "etape",
        alias = "Step",
        alias = "step"
    )]
    pub step: Option<u32>,
    #[serde(default, alias = "Numero", alias = "numero")]
    pub number: Option<u32>,
    #[serde(
        default,
        alias = "Actif",
        alias = "actif",
        alias = "Active",
        alias = "active",
        deserialize_with = "deserialize_optional_bool_flexible"
    )]
    pub active: Option<bool>,
    #[serde(
        default,
        alias = "Atteint",
        alias = "atteint",
        alias = "Status",
        alias = "status",
        deserialize_with = "deserialize_optional_bool_flexible"
    )]
    pub reached: Option<bool>,
}

impl MondialRelayStep {
    pub fn rank(&self, idx: usize) -> u32 {
        self.step
            .or(self.number)
            .unwrap_or_else(|| (idx + 1) as u32)
    }

    pub fn as_text(&self) -> Option<&str> {
        self.label.as_deref().or(self.code.as_deref())
    }

    pub fn is_active(&self) -> bool {
        self.active.unwrap_or(false)
    }

    pub fn is_reached(&self) -> bool {
        self.reached.unwrap_or(false)
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct MondialRelayEvent {
    #[serde(
        default,
        alias = "Date",
        alias = "date",
        alias = "DateEvenement",
        alias = "dateEvenement"
    )]
    pub date: Option<String>,
    #[serde(
        default,
        alias = "Libelle",
        alias = "libelle",
        alias = "Description",
        alias = "description",
        alias = "Label",
        alias = "label"
    )]
    pub description: Option<String>,
    #[serde(
        default,
        alias = "Lieu",
        alias = "lieu",
        alias = "Localite",
        alias = "localite",
        alias = "Ville",
        alias = "ville"
    )]
    pub location: Option<String>,
    #[serde(
        default,
        alias = "Code",
        alias = "code",
        alias = "Etat",
        alias = "etat"
    )]
    pub code: Option<String>,
    #[serde(default, alias = "DetailPointRelais", alias = "detailPointRelais")]
    pub detail_point_relais: Option<MondialRelayPointRelais>,
}

impl MondialRelayEvent {
    pub fn resolved_location(&self) -> Option<String> {
        if let Some(address) = self
            .detail_point_relais
            .as_ref()
            .and_then(|relay| relay.address.as_ref())
        {
            let mut parts = Vec::new();

            for value in [
                address.label.as_deref(),
                address.line1.as_deref(),
                address.line2.as_deref(),
            ] {
                if let Some(trimmed) = value.map(str::trim).filter(|s| !s.is_empty()) {
                    parts.push(trimmed.to_string());
                }
            }

            let city_line = match (
                address.postcode.as_deref().map(str::trim),
                address.city.as_deref().map(str::trim),
            ) {
                (Some(postcode), Some(city)) if !postcode.is_empty() && !city.is_empty() => {
                    Some(format!("{postcode} {city}"))
                }
                (Some(postcode), _) if !postcode.is_empty() => Some(postcode.to_string()),
                (_, Some(city)) if !city.is_empty() => Some(city.to_string()),
                _ => None,
            };

            if let Some(city_line) = city_line {
                parts.push(city_line);
            }

            if let Some(country) = address
                .country_code
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                parts.push(country.to_string());
            }

            if !parts.is_empty() {
                return Some(parts.join(", "));
            }
        }

        self.location
            .as_ref()
            .map(|location| location.trim().to_string())
            .filter(|location| !location.is_empty())
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct MondialRelayPointRelais {
    #[serde(default, alias = "Adresse", alias = "adresse")]
    pub address: Option<MondialRelayAddress>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct MondialRelayAddress {
    #[serde(default, alias = "Libelle", alias = "libelle")]
    pub label: Option<String>,
    #[serde(default, alias = "AdresseLigne1", alias = "adresseLigne1")]
    pub line1: Option<String>,
    #[serde(default, alias = "AdresseLigne2", alias = "adresseLigne2")]
    pub line2: Option<String>,
    #[serde(default, alias = "CodePostal", alias = "codePostal")]
    pub postcode: Option<String>,
    #[serde(default, alias = "Ville", alias = "ville")]
    pub city: Option<String>,
    #[serde(default, alias = "CodePays", alias = "codePays")]
    pub country_code: Option<String>,
}

fn deserialize_vec_or_map<'de, D, T>(deserializer: D) -> std::result::Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let value = serde_json::Value::deserialize(deserializer)?;

    match value {
        serde_json::Value::Null => Ok(Vec::new()),
        serde_json::Value::Array(items) => items
            .into_iter()
            .map(|item| serde_json::from_value(item).map_err(D::Error::custom))
            .collect(),
        serde_json::Value::Object(map) => {
            let as_object = serde_json::Value::Object(map.clone());
            if let Ok(single) = serde_json::from_value::<T>(as_object) {
                return Ok(vec![single]);
            }

            let mut values: Vec<(String, serde_json::Value)> = map.into_iter().collect();
            values.sort_by(|(left, _), (right, _)| {
                match (left.parse::<u32>(), right.parse::<u32>()) {
                    (Ok(l), Ok(r)) => l.cmp(&r),
                    _ => left.cmp(right),
                }
            });
            values
                .into_iter()
                .map(|(_, item)| serde_json::from_value(item).map_err(D::Error::custom))
                .collect()
        }
        other => serde_json::from_value::<T>(other)
            .map(|single| vec![single])
            .map_err(D::Error::custom),
    }
}

fn deserialize_optional_bool_flexible<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<bool>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;

    let parsed = match value {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::Bool(value)) => Some(value),
        Some(serde_json::Value::Number(value)) => value.as_i64().map(|int| int != 0),
        Some(serde_json::Value::String(value)) => {
            match value.trim().to_ascii_lowercase().as_str() {
                "1" | "true" | "yes" | "y" => Some(true),
                "0" | "false" | "no" | "n" => Some(false),
                _ => None,
            }
        }
        Some(serde_json::Value::Object(map)) => {
            if let Some(reached) = map.get("reached").and_then(serde_json::Value::as_bool) {
                Some(reached)
            } else if let Some(reached) = map.get("active").and_then(serde_json::Value::as_bool) {
                Some(reached)
            } else if let Some(code) = map.get("code").and_then(serde_json::Value::as_str) {
                let lower = code.to_ascii_lowercase();
                if lower == "ok" || lower == "reached" || lower == "done" {
                    Some(true)
                } else {
                    None
                }
            } else {
                None
            }
        }
        Some(serde_json::Value::Array(_)) => None,
    };

    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_events_from_object_map_shape() {
        let payload = r#"{
          "Expedition": {
            "Evenements": {
              "1": {"Date":"2026-01-02T17:32:32.765","Libelle":"A","Code":"X"},
              "2": {"Date":"2026-01-03T17:32:32.765","Libelle":"B","Code":"Y"}
            }
          }
        }"#;
        let parsed: MondialRelayResponse = serde_json::from_str(payload).expect("valid json");
        assert_eq!(parsed.events_recursive().len(), 2);
    }

    #[test]
    fn parses_steps_with_status_object_without_error() {
        let payload = r#"{
          "Expedition": {
            "SuiviParEtapes": [
              {"Libelle":"Colis remis","Status":{"code":"ok"}}
            ]
          }
        }"#;
        let parsed: MondialRelayResponse = serde_json::from_str(payload).expect("valid json");
        assert_eq!(parsed.steps_recursive().len(), 1);
    }

    #[test]
    fn resolves_location_from_detail_point_relais_address() {
        let payload = r#"{
          "Expedition": {
            "Evenements": [
              {
                "Date":"2025-10-23T13:09:13.476",
                "Libelle":"Colis livré au destinataire",
                "DetailPointRelais": {
                  "Adresse": {
                    "Libelle":"LOCKER EXEMPLE 00000 VILLE-TEST",
                    "AdresseLigne1":"10 RUE EXEMPLE",
                    "CodePostal":"00000",
                    "Ville":"CITY-TEST",
                    "CodePays":"FR"
                  }
                }
              }
            ]
          }
        }"#;
        let parsed: MondialRelayResponse = serde_json::from_str(payload).expect("valid json");
        let event = parsed.events_recursive().first().expect("one event");
        let location = event.resolved_location().expect("resolved location");
        assert!(location.contains("LOCKER EXEMPLE 00000 VILLE-TEST"));
        assert!(location.contains("10 RUE EXEMPLE"));
        assert!(location.contains("00000 CITY-TEST"));
    }
}
