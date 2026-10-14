//! Protocolo de mensajes de DameBola (v1). Ver docs/NETWORK_PROTOCOL.md.
//!
//! Cada mensaje es un frame WebSocket de texto con JSON:
//! `{"v":1,"id":"<uuid>","type":"ALERT_REQUEST","body":{}}`
//!
//! La validación es estricta: tamaño máximo, versión, claves exactas,
//! `deny_unknown_fields` en cada cuerpo y límites de longitud en los textos.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use uuid::Uuid;

pub const PROTOCOL_VERSION: u32 = 1;
/// Tamaño máximo de un mensaje (también se aplica en el frame WebSocket).
pub const MAX_MESSAGE_BYTES: usize = 4096;
pub const MAX_NAME_CHARS: usize = 40;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProtocolError {
    #[error("mensaje demasiado grande")]
    TooLarge,
    #[error("JSON inválido")]
    Malformed,
    #[error("versión de protocolo no soportada: {0}")]
    UnsupportedVersion(u64),
    #[error("identificador de mensaje inválido")]
    BadId,
    #[error("campos inesperados o faltantes")]
    BadShape,
    #[error("tipo de mensaje desconocido o cuerpo inválido")]
    BadBody,
    #[error("datos inválidos: {0}")]
    Invalid(&'static str),
}

/// Cuerpo vacío estricto: solo acepta `{}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Empty {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Hello {
    pub device_id: String,
    pub name: String,
    pub os: String,
    pub app_version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AckStage {
    /// El destinatario validó al remitente y las reglas; va a mostrar la alerta.
    Accepted,
    /// Las ventanas de alerta ya se crearon en el destinatario.
    Shown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectReason {
    Unauthorized,
    RateLimited,
    Muted,
    Disabled,
    Busy,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairRejectReason {
    Declined,
    Mismatch,
    Busy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AlertAck {
    pub reply_to: Uuid,
    pub stage: AckStage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AlertReject {
    pub reply_to: Uuid,
    pub reason: RejectReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Pong {
    pub reply_to: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairReject {
    pub reason: PairRejectReason,
}

/// Cuerpo de un mensaje. Una alerta NO lleva datos: el remitente es la
/// identidad autenticada por TLS, nunca un campo del mensaje.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "body", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Body {
    Hello(Hello),
    PairRequest(Empty),
    PairAccept(Empty),
    PairConfirm(Empty),
    PairReject(PairReject),
    Ping(Empty),
    Pong(Pong),
    AlertRequest(Empty),
    AlertAck(AlertAck),
    AlertReject(AlertReject),
    Unpair(Empty),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub id: Uuid,
    pub body: Body,
}

impl Message {
    pub fn new(body: Body) -> Self {
        Self { id: Uuid::new_v4(), body }
    }

    pub fn encode(&self) -> String {
        let mut obj = match serde_json::to_value(&self.body) {
            Ok(Value::Object(m)) => m,
            _ => Map::new(),
        };
        obj.insert("v".into(), json!(PROTOCOL_VERSION));
        obj.insert("id".into(), json!(self.id.to_string()));
        Value::Object(obj).to_string()
    }

    /// Decodifica y valida un mensaje recibido (no confiable).
    pub fn decode(text: &str) -> Result<Self, ProtocolError> {
        if text.len() > MAX_MESSAGE_BYTES {
            return Err(ProtocolError::TooLarge);
        }
        let Value::Object(mut obj) = serde_json::from_str::<Value>(text).map_err(|_| ProtocolError::Malformed)? else {
            return Err(ProtocolError::BadShape);
        };
        if obj.keys().any(|k| !matches!(k.as_str(), "v" | "id" | "type" | "body")) {
            return Err(ProtocolError::BadShape);
        }
        let (Some(v), Some(id), Some(ty), Some(body)) =
            (obj.remove("v"), obj.remove("id"), obj.remove("type"), obj.remove("body"))
        else {
            return Err(ProtocolError::BadShape);
        };
        let version = v.as_u64().ok_or(ProtocolError::BadShape)?;
        if version != u64::from(PROTOCOL_VERSION) {
            return Err(ProtocolError::UnsupportedVersion(version));
        }
        let id = id
            .as_str()
            .filter(|s| s.len() == 36)
            .and_then(|s| Uuid::parse_str(s).ok())
            .ok_or(ProtocolError::BadId)?;
        let body: Body = serde_json::from_value(json!({ "type": ty, "body": body })).map_err(|_| ProtocolError::BadBody)?;
        if let Body::Hello(h) = &body {
            validate_hello(h)?;
        }
        Ok(Self { id, body })
    }
}

fn validate_hello(h: &Hello) -> Result<(), ProtocolError> {
    if !is_device_id(&h.device_id) {
        return Err(ProtocolError::Invalid("deviceId"));
    }
    if sanitize_name(&h.name).as_deref() != Some(h.name.as_str()) {
        return Err(ProtocolError::Invalid("name"));
    }
    if !matches!(h.os.as_str(), "macos" | "windows" | "linux" | "other") {
        return Err(ProtocolError::Invalid("os"));
    }
    if h.app_version.is_empty() || h.app_version.len() > 32 || !h.app_version.chars().all(|c| c.is_ascii_alphanumeric() || ".-+".contains(c)) {
        return Err(ProtocolError::Invalid("appVersion"));
    }
    Ok(())
}

/// 32 caracteres hexadecimales en minúscula.
pub fn is_device_id(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Normaliza un nombre visible recibido de la red: sin controles, recortado,
/// 1..=40 caracteres. Devuelve `None` si queda vacío.
pub fn sanitize_name(raw: &str) -> Option<String> {
    let cleaned: String = raw.chars().filter(|c| !c.is_control()).collect();
    let cleaned = cleaned.trim();
    let cleaned: String = cleaned.chars().take(MAX_NAME_CHARS).collect();
    let cleaned = cleaned.trim().to_string();
    (!cleaned.is_empty()).then_some(cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hello() -> Hello {
        Hello { device_id: "a".repeat(32), name: "Ana".into(), os: "macos".into(), app_version: "0.2.0".into() }
    }

    fn roundtrip(body: Body) {
        let m = Message::new(body);
        assert_eq!(Message::decode(&m.encode()).unwrap(), m);
    }

    #[test]
    fn every_message_type_roundtrips() {
        let id = Uuid::new_v4();
        for body in [
            Body::Hello(hello()),
            Body::PairRequest(Empty {}),
            Body::PairAccept(Empty {}),
            Body::PairConfirm(Empty {}),
            Body::PairReject(PairReject { reason: PairRejectReason::Declined }),
            Body::Ping(Empty {}),
            Body::Pong(Pong { reply_to: id }),
            Body::AlertRequest(Empty {}),
            Body::AlertAck(AlertAck { reply_to: id, stage: AckStage::Shown }),
            Body::AlertReject(AlertReject { reply_to: id, reason: RejectReason::RateLimited }),
            Body::Unpair(Empty {}),
        ] {
            roundtrip(body);
        }
    }

    #[test]
    fn wire_format_is_stable() {
        let m = Message { id: Uuid::nil(), body: Body::AlertRequest(Empty {}) };
        let v: Value = serde_json::from_str(&m.encode()).unwrap();
        assert_eq!(v, json!({"v":1,"id":"00000000-0000-0000-0000-000000000000","type":"ALERT_REQUEST","body":{}}));
        let ack = Message { id: Uuid::nil(), body: Body::AlertAck(AlertAck { reply_to: Uuid::nil(), stage: AckStage::Accepted }) };
        let v: Value = serde_json::from_str(&ack.encode()).unwrap();
        assert_eq!(v["type"], "ALERT_ACK");
        assert_eq!(v["body"]["stage"], "accepted");
        assert_eq!(v["body"]["replyTo"], "00000000-0000-0000-0000-000000000000");
    }

    const ID: &str = "00000000-0000-0000-0000-000000000000";

    #[test]
    fn rejects_malformed_and_oversized() {
        assert_eq!(Message::decode("no json"), Err(ProtocolError::Malformed));
        assert_eq!(Message::decode("[]"), Err(ProtocolError::BadShape));
        assert_eq!(Message::decode(&" ".repeat(MAX_MESSAGE_BYTES + 1)), Err(ProtocolError::TooLarge));
    }

    #[test]
    fn rejects_wrong_version_and_missing_or_extra_keys() {
        let ok = |extra: &str| format!(r#"{{"v":1,"id":"{ID}","type":"PING","body":{{}}{extra}}}"#);
        assert!(Message::decode(&ok("")).is_ok());
        assert_eq!(Message::decode(&ok(r#","x":1"#)), Err(ProtocolError::BadShape));
        assert_eq!(
            Message::decode(&format!(r#"{{"v":2,"id":"{ID}","type":"PING","body":{{}}}}"#)),
            Err(ProtocolError::UnsupportedVersion(2))
        );
        assert_eq!(Message::decode(&format!(r#"{{"v":1,"id":"{ID}","type":"PING"}}"#)), Err(ProtocolError::BadShape));
        assert_eq!(Message::decode(r#"{"v":1,"id":"x","type":"PING","body":{}}"#), Err(ProtocolError::BadId));
        assert_eq!(Message::decode(r#"{"v":1,"id":7,"type":"PING","body":{}}"#), Err(ProtocolError::BadId));
    }

    #[test]
    fn rejects_unknown_type_and_unknown_body_fields() {
        let m = |ty: &str, body: &str| format!(r#"{{"v":1,"id":"{ID}","type":"{ty}","body":{body}}}"#);
        assert_eq!(Message::decode(&m("RUN_COMMAND", "{}")), Err(ProtocolError::BadBody));
        // Una alerta no puede cargar datos extra (p. ej. rutas de archivos).
        assert_eq!(Message::decode(&m("ALERT_REQUEST", r#"{"path":"/etc/passwd"}"#)), Err(ProtocolError::BadBody));
        assert_eq!(Message::decode(&m("PING", "null")), Err(ProtocolError::BadBody));
        assert_eq!(
            Message::decode(&m("ALERT_ACK", &format!(r#"{{"replyTo":"{ID}","stage":"hacked"}}"#))),
            Err(ProtocolError::BadBody)
        );
    }

    #[test]
    fn validates_hello_fields() {
        let enc = |h: Hello| Message::new(Body::Hello(h)).encode();
        assert!(Message::decode(&enc(hello())).is_ok());
        assert!(Message::decode(&enc(Hello { device_id: "XYZ".into(), ..hello() })).is_err());
        assert!(Message::decode(&enc(Hello { device_id: "A".repeat(32), ..hello() })).is_err());
        assert!(Message::decode(&enc(Hello { name: "  ".into(), ..hello() })).is_err());
        assert!(Message::decode(&enc(Hello { name: "x".repeat(41), ..hello() })).is_err());
        assert!(Message::decode(&enc(Hello { name: "a\nb".into(), ..hello() })).is_err());
        assert!(Message::decode(&enc(Hello { os: "plan9".into(), ..hello() })).is_err());
        assert!(Message::decode(&enc(Hello { app_version: "<script>".into(), ..hello() })).is_err());
    }

    #[test]
    fn sanitize_name_cleans_input() {
        assert_eq!(sanitize_name("  Ana\u{0007} "), Some("Ana".into()));
        assert_eq!(sanitize_name("\n\t"), None);
        assert_eq!(sanitize_name(&"é".repeat(100)).unwrap().chars().count(), 40);
    }
}
