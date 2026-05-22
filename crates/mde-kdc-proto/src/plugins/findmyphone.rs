//! KDC2-2.10 findmyphone plugin — `kdeconnect.findmyphone.request`.
//!
//! Body is empty — receipt of the packet itself is the signal to
//! ring. The `.request` suffix is upstream's convention for
//! action-trigger packets.

use serde::{Deserialize, Serialize};

use crate::wire::Packet;

/// `kdeconnect.findmyphone.request` body — empty by design.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FindMyPhoneBody;

/// Build a findmyphone trigger packet.
#[must_use]
pub fn find_my_phone_packet(id_ms: i64) -> Packet {
    Packet {
        id: id_ms,
        kind: "kdeconnect.findmyphone.request".to_string(),
        body: serde_json::json!({}),
        mde_caps: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn findmyphone_packet_kind_includes_request_suffix() {
        let p = find_my_phone_packet(1);
        assert_eq!(p.kind, "kdeconnect.findmyphone.request");
        assert_eq!(
            p.kind,
            crate::plugins::PluginKind::FindMyPhone.packet_kind(),
        );
    }

    #[test]
    fn findmyphone_body_serializes_as_empty_object() {
        let p = find_my_phone_packet(1);
        let s = serde_json::to_string(&p).unwrap();
        // Body is `{}` — the trigger semantic is "packet arrived,
        // ring the phone." No metadata needed.
        assert!(s.contains(r#""body":{}"#));
    }

    #[test]
    fn findmyphone_body_round_trips_via_wire() {
        let p = find_my_phone_packet(42);
        let wire = serde_json::to_string(&p).unwrap();
        let decoded: Packet = serde_json::from_str(&wire).unwrap();
        assert_eq!(decoded.id, 42);
        assert_eq!(decoded.kind, "kdeconnect.findmyphone.request");
    }
}
