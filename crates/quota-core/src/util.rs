use base64::Engine;
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use serde_json::Value;

/// Accepts RFC 3339 strings, plain dates, and epoch seconds or milliseconds (as numbers or strings).
pub fn parse_time(v: &Value) -> Option<DateTime<Utc>> {
    match v {
        Value::Number(n) => n.as_f64().and_then(epoch_to_time),
        Value::String(s) => {
            let s = s.trim();
            if s.is_empty() {
                return None;
            }
            if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
                return Some(dt.with_timezone(&Utc));
            }
            if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
                return Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0)?).into();
            }
            if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f") {
                return Some(Utc.from_utc_datetime(&dt));
            }
            s.parse::<f64>().ok().and_then(epoch_to_time)
        }
        _ => None,
    }
}

fn epoch_to_time(x: f64) -> Option<DateTime<Utc>> {
    if !x.is_finite() || x <= 0.0 {
        return None;
    }
    // Anything past year ~2286 in seconds is really milliseconds.
    let secs = if x > 1e10 { x / 1000.0 } else { x };
    Utc.timestamp_opt(secs as i64, 0).single()
}

pub fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// First present numeric field among `keys`.
pub fn num_at(obj: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|k| obj.get(*k).and_then(num))
}

pub fn str_at<'a>(obj: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|k| obj.get(*k).and_then(Value::as_str))
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

pub fn time_at(obj: &Value, keys: &[&str]) -> Option<DateTime<Utc>> {
    keys.iter().find_map(|k| obj.get(*k).and_then(parse_time))
}

/// Decodes a JWT payload without verifying it (only used to read `exp`, `sub`, `email`).
pub fn jwt_claims(token: &str) -> Option<Value> {
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn pct(used: f64, limit: f64) -> f64 {
    if limit > 0.0 {
        used / limit * 100.0
    } else {
        0.0
    }
}

/// "pro" -> "Pro", "free_workspace" -> "Free Workspace".
pub fn title_case(s: &str) -> String {
    s.split(['_', '-', ' '])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_time_shapes() {
        let a = parse_time(&json!("2025-11-08T10:00:00Z")).unwrap();
        let b = parse_time(&json!(1762596000)).unwrap();
        let c = parse_time(&json!(1762596000000_i64)).unwrap();
        let d = parse_time(&json!("1762596000")).unwrap();
        assert_eq!(a, b);
        assert_eq!(b, c);
        assert_eq!(c, d);
        assert_eq!(parse_time(&json!("2025-12-01")).unwrap().to_rfc3339(), "2025-12-01T00:00:00+00:00");
        assert!(parse_time(&json!(null)).is_none());
        assert!(parse_time(&json!(0)).is_none());
    }

    #[test]
    fn decodes_jwt_payload() {
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(br#"{"sub":"auth0|user_1","exp":1}"#);
        let token = format!("x.{payload}.y");
        assert_eq!(jwt_claims(&token).unwrap()["sub"], "auth0|user_1");
    }

    #[test]
    fn titles() {
        assert_eq!(title_case("free_workspace"), "Free Workspace");
        assert_eq!(title_case("pro"), "Pro");
    }
}
