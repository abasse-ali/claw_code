//! Encodage protobuf des messages OTLP (`ExportMetricsServiceRequest`,
//! `ExportLogsServiceRequest`) à partir de leur forme JSON, pour les
//! protocoles `http/protobuf` et `grpc`. Les numéros de champs sont ceux
//! d'opentelemetry-proto ; seuls les champs que Claw produit sont écrits.

use serde_json::Value;

const VARINT: u64 = 0;
const FIXED64: u64 = 1;
const LENGTH: u64 = 2;

fn varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = u8::try_from(value & 0x7f).unwrap_or_default();
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn tag(out: &mut Vec<u8>, field: u64, wire: u64) {
    varint(out, (field << 3) | wire);
}

fn bytes(out: &mut Vec<u8>, field: u64, data: &[u8]) {
    tag(out, field, LENGTH);
    varint(out, u64::try_from(data.len()).unwrap_or(u64::MAX));
    out.extend_from_slice(data);
}

fn text(out: &mut Vec<u8>, field: u64, value: &str) {
    if !value.is_empty() {
        bytes(out, field, value.as_bytes());
    }
}

fn unsigned(out: &mut Vec<u8>, field: u64, value: u64) {
    if value != 0 {
        tag(out, field, VARINT);
        varint(out, value);
    }
}

fn fixed64(out: &mut Vec<u8>, field: u64, value: u64) {
    if value != 0 {
        tag(out, field, FIXED64);
        out.extend_from_slice(&value.to_le_bytes());
    }
}

fn double(out: &mut Vec<u8>, field: u64, value: f64) {
    tag(out, field, FIXED64);
    out.extend_from_slice(&value.to_le_bytes());
}

/// Nombre écrit en texte (`"123"`, convention OTLP/JSON pour 64 bits) ou en nombre.
fn number(value: &Value) -> u64 {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
        .unwrap_or(0)
}

/// `AnyValue` : 1 chaîne, 2 booléen, 3 entier, 4 double.
fn any_value(value: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    if let Some(text) = value.get("stringValue").and_then(Value::as_str) {
        bytes(&mut out, 1, text.as_bytes());
    } else if let Some(flag) = value.get("boolValue").and_then(Value::as_bool) {
        tag(&mut out, 2, VARINT);
        varint(&mut out, u64::from(flag));
    } else if let Some(integer) = value.get("intValue") {
        let integer = integer
            .as_i64()
            .or_else(|| integer.as_str().and_then(|text| text.parse().ok()))
            .unwrap_or(0);
        tag(&mut out, 3, VARINT);
        // int64 : le complément à deux tient sur 64 bits.
        varint(&mut out, u64::from_ne_bytes(integer.to_ne_bytes()));
    } else if let Some(float) = value.get("doubleValue").and_then(Value::as_f64) {
        double(&mut out, 4, float);
    }
    out
}

/// `KeyValue` : 1 clé, 2 valeur.
fn key_value(entry: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    text(&mut out, 1, entry["key"].as_str().unwrap_or_default());
    bytes(&mut out, 2, &any_value(&entry["value"]));
    out
}

fn attributes(out: &mut Vec<u8>, field: u64, list: &Value) {
    for entry in list.as_array().into_iter().flatten() {
        bytes(out, field, &key_value(entry));
    }
}

/// `Resource` : 1 attributs.
fn resource(value: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    attributes(&mut out, 1, &value["attributes"]);
    out
}

/// `InstrumentationScope` : 1 nom, 2 version.
fn scope(value: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    text(&mut out, 1, value["name"].as_str().unwrap_or_default());
    text(&mut out, 2, value["version"].as_str().unwrap_or_default());
    out
}

/// `NumberDataPoint` : 7 attributs, 2 début, 3 instant, 4 valeur double.
fn number_point(point: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    fixed64(&mut out, 2, number(&point["startTimeUnixNano"]));
    fixed64(&mut out, 3, number(&point["timeUnixNano"]));
    double(&mut out, 4, point["asDouble"].as_f64().unwrap_or(0.0));
    attributes(&mut out, 7, &point["attributes"]);
    out
}

/// `Metric` : 1 nom, 3 unité, 7 `Sum` (1 points, 2 temporalité, 3 monotone).
fn metric(value: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    text(&mut out, 1, value["name"].as_str().unwrap_or_default());
    text(&mut out, 3, value["unit"].as_str().unwrap_or_default());
    let sum = &value["sum"];
    if sum.is_object() {
        let mut encoded = Vec::new();
        for point in sum["dataPoints"].as_array().into_iter().flatten() {
            bytes(&mut encoded, 1, &number_point(point));
        }
        unsigned(&mut encoded, 2, number(&sum["aggregationTemporality"]));
        if sum["isMonotonic"].as_bool() == Some(true) {
            tag(&mut encoded, 3, VARINT);
            varint(&mut encoded, 1);
        }
        bytes(&mut out, 7, &encoded);
    }
    out
}

/// `LogRecord` : 1 instant, 11 instant observé, 2 gravité, 3 texte de
/// gravité, 5 corps, 6 attributs.
fn log_record(value: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    fixed64(&mut out, 1, number(&value["timeUnixNano"]));
    unsigned(&mut out, 2, number(&value["severityNumber"]));
    text(
        &mut out,
        3,
        value["severityText"].as_str().unwrap_or_default(),
    );
    if value["body"].is_object() {
        bytes(&mut out, 5, &any_value(&value["body"]));
    }
    attributes(&mut out, 6, &value["attributes"]);
    fixed64(&mut out, 11, number(&value["observedTimeUnixNano"]));
    out
}

/// `resource*` (1 ressource, 2 `scope*`) ; `scope*` (1 portée, 2 éléments).
fn resource_blocks(
    payload: &Value,
    resources_key: &str,
    scopes_key: &str,
    items_key: &str,
    item: fn(&Value) -> Vec<u8>,
) -> Vec<u8> {
    let mut out = Vec::new();
    for block in payload[resources_key].as_array().into_iter().flatten() {
        let mut encoded = Vec::new();
        bytes(&mut encoded, 1, &resource(&block["resource"]));
        for scoped in block[scopes_key].as_array().into_iter().flatten() {
            let mut inner = Vec::new();
            bytes(&mut inner, 1, &scope(&scoped["scope"]));
            for entry in scoped[items_key].as_array().into_iter().flatten() {
                bytes(&mut inner, 2, &item(entry));
            }
            bytes(&mut encoded, 2, &inner);
        }
        bytes(&mut out, 1, &encoded);
    }
    out
}

/// `ExportMetricsServiceRequest` depuis sa forme OTLP/JSON.
#[must_use]
pub fn metrics_request(payload: &Value) -> Vec<u8> {
    resource_blocks(
        payload,
        "resourceMetrics",
        "scopeMetrics",
        "metrics",
        metric,
    )
}

/// `ExportLogsServiceRequest` depuis sa forme OTLP/JSON.
#[must_use]
pub fn logs_request(payload: &Value) -> Vec<u8> {
    resource_blocks(
        payload,
        "resourceLogs",
        "scopeLogs",
        "logRecords",
        log_record,
    )
}

#[cfg(test)]
mod tests {
    use super::{any_value, logs_request, metrics_request, varint};
    use serde_json::json;

    #[test]
    fn varints_and_values_follow_the_wire_format() {
        let mut out = Vec::new();
        varint(&mut out, 300);
        assert_eq!(out, vec![0xac, 0x02]);
        assert_eq!(
            any_value(&json!({ "stringValue": "a" })),
            vec![0x0a, 1, b'a']
        );
        assert_eq!(any_value(&json!({ "boolValue": true })), vec![0x10, 1]);
        assert_eq!(any_value(&json!({ "intValue": "-1" })).len(), 11);
    }

    #[test]
    fn requests_nest_resources_scopes_and_items() {
        let metrics = metrics_request(&json!({
            "resourceMetrics": [{
                "resource": { "attributes": [{ "key": "service.name", "value": { "stringValue": "claw-code" } }] },
                "scopeMetrics": [{
                    "scope": { "name": "claw_code", "version": "1" },
                    "metrics": [{ "name": "claw_code.session.count", "unit": "", "sum": {
                        "aggregationTemporality": 1, "isMonotonic": true,
                        "dataPoints": [{ "attributes": [], "startTimeUnixNano": "1", "timeUnixNano": "2", "asDouble": 1.0 }]
                    } }]
                }]
            }]
        }));
        // resource_metrics (champ 1, longueur), puis le nom de la métrique.
        assert_eq!(metrics[0], 0x0a);
        assert!(metrics
            .windows(b"claw_code.session.count".len())
            .any(|window| window == b"claw_code.session.count"));
        let logs = logs_request(&json!({
            "resourceLogs": [{ "resource": { "attributes": [] }, "scopeLogs": [{
                "scope": { "name": "claw_code" },
                "logRecords": [{ "timeUnixNano": "5", "severityNumber": 9, "body": { "stringValue": "x" }, "attributes": [] }]
            }] }]
        }));
        assert_eq!(logs[0], 0x0a);
        assert!(logs.contains(&b'x'));
    }
}
