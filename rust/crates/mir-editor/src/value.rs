//! Text forms of MirDB values for the detail form, and the typed cell widgets.

use eframe::egui;
use mir_formats::mirdb::Value;

/// Short one-line rendering for table cells.
pub fn summary(v: &Value) -> String {
    match v {
        Value::Bool(b) => if *b { "✔" } else { "" }.to_string(),
        Value::Int(i) => i.to_string(),
        Value::UInt(u) => u.to_string(),
        Value::Float(f) => format!("{f}"),
        Value::Decimal(..) => format!("{}", v.as_f64().unwrap_or(0.0)),
        Value::Str(s) => s.clone(),
        Value::Bytes(b) => format!("{} bytes", b.len()),
        Value::Point(x, y) => format!("{x},{y}"),
        Value::Size(w, h) => format!("{w}x{h}"),
        Value::Color(c) => format!("#{c:08X}"),
        Value::DateTime(t) => t.to_string(),
        Value::TimeSpan(t) => t.to_string(),
        Value::IntArray(None) | Value::PointArray(None) | Value::BitArray(None) => String::new(),
        Value::Stats(None) => String::new(),
        Value::IntArray(Some(a)) => a
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(","),
        Value::PointArray(Some(a)) => format!("{} points", a.len()),
        Value::BitArray(Some(b)) => format!("{} bytes", b.len()),
        Value::Stats(Some(s)) => s
            .iter()
            .map(|(k, v)| format!("{k}:{v}"))
            .collect::<Vec<_>>()
            .join(";"),
    }
}

/// Editable text form (used for the types that have no direct widget).
pub fn to_text(v: &Value) -> String {
    match v {
        Value::Bytes(b) | Value::BitArray(Some(b)) => {
            b.iter().map(|x| format!("{x:02x}")).collect::<String>()
        }
        Value::PointArray(Some(a)) => a
            .iter()
            .map(|(x, y)| format!("{x},{y}"))
            .collect::<Vec<_>>()
            .join(";"),
        Value::Str(s) => s.clone(),
        _ => summary(v),
    }
}

fn hex_bytes(s: &str) -> Option<Vec<u8>> {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

fn pair(s: &str) -> Option<(i32, i32)> {
    let (a, b) = s.split_once([',', 'x'])?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

/// Parse `text` into the same variant as `like`; `None` when it does not fit.
pub fn from_text(like: &Value, text: &str) -> Option<Value> {
    let t = text.trim();
    Some(match like {
        Value::Bool(_) => Value::Bool(matches!(t, "1" | "true" | "yes" | "✔")),
        Value::Int(_) => Value::Int(t.parse().ok()?),
        Value::UInt(_) => Value::UInt(t.parse().ok()?),
        Value::Float(_) => Value::Float(t.parse().ok()?),
        Value::Decimal(..) => {
            // Store as scaled integer with up to 4 decimals (.NET decimal layout).
            let f: f64 = t.parse().ok()?;
            let scaled = (f.abs() * 10_000.0).round() as u128;
            let flags = (4u32 << 16) | if f < 0.0 { 0x8000_0000 } else { 0 };
            Value::Decimal(
                scaled as u32,
                (scaled >> 32) as u32,
                (scaled >> 64) as u32,
                flags,
            )
        }
        Value::Str(_) => Value::Str(text.to_string()),
        Value::Bytes(_) => Value::Bytes(hex_bytes(t)?),
        Value::Point(..) => {
            let (x, y) = pair(t)?;
            Value::Point(x, y)
        }
        Value::Size(..) => {
            let (w, h) = pair(t)?;
            Value::Size(w, h)
        }
        Value::Color(_) => Value::Color(u32::from_str_radix(t.trim_start_matches('#'), 16).ok()?),
        Value::DateTime(_) => Value::DateTime(t.parse().ok()?),
        Value::TimeSpan(_) => Value::TimeSpan(t.parse().ok()?),
        Value::IntArray(_) => {
            if t.is_empty() {
                Value::IntArray(None)
            } else {
                Value::IntArray(Some(
                    t.split(',')
                        .map(|x| x.trim().parse().ok())
                        .collect::<Option<Vec<i32>>>()?,
                ))
            }
        }
        Value::PointArray(_) => {
            if t.is_empty() {
                Value::PointArray(None)
            } else {
                Value::PointArray(Some(t.split(';').map(pair).collect::<Option<Vec<_>>>()?))
            }
        }
        Value::BitArray(_) => {
            if t.is_empty() {
                Value::BitArray(None)
            } else {
                Value::BitArray(Some(hex_bytes(t)?))
            }
        }
        Value::Stats(_) => {
            if t.is_empty() {
                Value::Stats(None)
            } else {
                Value::Stats(Some(
                    t.split(';')
                        .filter(|x| !x.trim().is_empty())
                        .map(|x| {
                            let (k, v) = x.split_once(':')?;
                            Some((k.trim().parse().ok()?, v.trim().parse().ok()?))
                        })
                        .collect::<Option<Vec<_>>>()?,
                ))
            }
        }
    })
}

/// Direct widget for the simple types; returns true when the value changed.
/// Complex types render their summary and return false (edit them in the form).
pub fn cell_widget(ui: &mut egui::Ui, v: &mut Value, compact: bool) -> bool {
    match v {
        Value::Bool(b) => ui.checkbox(b, "").changed(),
        Value::Int(i) => ui.add(egui::DragValue::new(i).speed(1.0)).changed(),
        Value::UInt(u) => ui.add(egui::DragValue::new(u).speed(1.0)).changed(),
        Value::Float(f) => ui.add(egui::DragValue::new(f).speed(0.1)).changed(),
        Value::Str(s) => {
            if compact {
                ui.add(egui::TextEdit::singleline(s).desired_width(f32::INFINITY))
                    .changed()
            } else {
                ui.add(
                    egui::TextEdit::multiline(s)
                        .desired_rows(1)
                        .desired_width(f32::INFINITY),
                )
                .changed()
            }
        }
        other => {
            ui.label(summary(other));
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_round_trips_complex_values() {
        let cases = vec![
            Value::Stats(Some(vec![(2, 10), (8, 3)])),
            Value::IntArray(Some(vec![1, 2, 3])),
            Value::IntArray(None),
            Value::PointArray(Some(vec![(1, 2), (3, 4)])),
            Value::Bytes(vec![0xde, 0xad]),
            Value::Point(5, -6),
            Value::Size(48, 32),
            Value::Color(0xFF00FF80),
            Value::Decimal(125, 0, 0, 1 << 16),
        ];
        for v in cases {
            let back = from_text(&v, &to_text(&v)).unwrap_or_else(|| panic!("{v:?}"));
            match (&v, &back) {
                (Value::Decimal(..), Value::Decimal(..)) => {
                    assert_eq!(v.as_f64(), back.as_f64());
                }
                _ => assert_eq!(v, back),
            }
        }
        assert!(from_text(&Value::Int(0), "abc").is_none());
    }
}
