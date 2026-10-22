use serde_json::Value;
use xm_core::command::Table;
use xm_core::event::{parse, DeviceEvent, Wearing};

fn events(jsonl: &str) -> Vec<DeviceEvent> {
    let mut out = Vec::new();
    for line in jsonl.lines().filter(|l| !l.trim().is_empty()) {
        let o: Value = serde_json::from_str(line).unwrap();
        if o["dir"] != "rx" {
            continue;
        }
        let table = if o["table"] == 2 {
            Table::Two
        } else {
            Table::One
        };
        let bytes: Vec<u8> = o["bytes"]
            .as_str()
            .unwrap()
            .split_whitespace()
            .map(|h| u8::from_str_radix(h, 16).unwrap())
            .collect();
        out.extend(parse(table, &bytes));
    }
    out
}

#[test]
fn captures_parse_without_panic() {
    let connect = events(include_str!(
        "../../../docs/protocol/captures/2026-10-19-wh-1000xm6-connect.jsonl"
    ));
    assert!(connect
        .iter()
        .any(|e| matches!(e, DeviceEvent::ProtocolInfo { .. })));
    assert!(connect
        .iter()
        .any(|e| matches!(e, DeviceEvent::SupportFunctions(_))));
    assert!(connect
        .iter()
        .any(|e| matches!(e, DeviceEvent::NcAmb { .. })));
    assert!(connect
        .iter()
        .any(|e| matches!(e, DeviceEvent::Battery { .. })));
    assert!(connect
        .iter()
        .any(|e| matches!(e, DeviceEvent::DeviceList(d) if d.len() == 6)));
    assert!(connect
        .iter()
        .any(|e| matches!(e, DeviceEvent::NowPlaying { title: Some(_), .. })));

    let wearing = events(include_str!(
        "../../../docs/protocol/captures/2026-10-19-wh-1000xm6-wearing.jsonl"
    ));
    assert!(wearing.contains(&DeviceEvent::Wearing(Wearing::Removed)));
}
