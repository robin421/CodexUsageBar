use quota_core::ctx::Ctx;
use quota_core::error::FetchError;
use quota_core::providers::agy::update_accounts_payload;
use quota_core::providers::claude;
use serde_json::Value;

#[test]
fn test_update_accounts_payload_invalid_json() {
    let invalid_inputs = [
        "not json at all",
        "{malformed: true,",
        "{",
        "}",
        "12345",
        "true",
        "\"string only\"",
        "[]",
        "{\"active\": 123}",
        "{\"old\": \"not an array\"}",
        "{\"active\": null, \"old\": null}",
    ];

    for input in invalid_inputs {
        let payload = update_accounts_payload(Some(input), "new_user@example.com");
        assert_eq!(
            payload["active"], "new_user@example.com",
            "active must be set to new_user even for invalid JSON: {input}"
        );
        assert!(
            payload["old"].is_array(),
            "old must always be an array even for invalid JSON: {input}"
        );
    }
}

#[test]
fn test_update_accounts_payload_empty_and_whitespace_strings() {
    let empty_inputs = [
        "",
        "   ",
        "\t\n\r",
        "{\"active\": \"\", \"old\": [\"\", \"   \", \"\t\"]}",
    ];

    for input in empty_inputs {
        let payload = update_accounts_payload(Some(input), "user@example.com");
        assert_eq!(payload["active"], "user@example.com");
        let old: Vec<String> = serde_json::from_value(payload["old"].clone()).unwrap();
        assert!(
            old.is_empty(),
            "empty string entries must not be included in old accounts"
        );
    }
}

#[test]
fn test_update_accounts_payload_large_payload() {
    // 5,000 accounts with duplicates
    let mut accounts = Vec::new();
    for i in 0..5000 {
        accounts.push(format!("account_{}@example.com", i % 500));
    }
    let json_str = serde_json::json!({
        "active": "account_0@example.com",
        "old": accounts
    }).to_string();

    let payload = update_accounts_payload(Some(&json_str), "target_active@example.com");
    assert_eq!(payload["active"], "target_active@example.com");
    let old: Vec<String> = serde_json::from_value(payload["old"].clone()).unwrap();
    // Unique accounts should be 500 (account_0 to account_499)
    assert_eq!(old.len(), 500, "all duplicates must be deduplicated efficiently");
    // Ensure no duplicates in old
    let set: std::collections::HashSet<_> = old.iter().collect();
    assert_eq!(set.len(), old.len());
    // target_active should not be in old
    assert!(!old.contains(&"target_active@example.com".to_string()));
}

#[test]
fn test_update_accounts_payload_special_characters_and_unicode() {
    let special_emails = [
        "user+tag@example.com",
        "张三@公司.中国",
        "techno_admin@sub-domain.co.uk",
        "emoji_🚀@universal.ai",
        "special!#$%&'*+-/=?^_`{|}~@example.com",
    ];

    let mut current_json = None;
    for email in special_emails {
        let payload = update_accounts_payload(current_json.as_deref(), email);
        assert_eq!(payload["active"], email);
        current_json = Some(payload.to_string());
    }

    let final_payload: Value = serde_json::from_str(&current_json.unwrap()).unwrap();
    assert_eq!(final_payload["active"], "special!#$%&'*+-/=?^_`{|}~@example.com");
    let old: Vec<String> = serde_json::from_value(final_payload["old"].clone()).unwrap();
    assert_eq!(old.len(), 4);
    assert_eq!(old[0], "emoji_🚀@universal.ai");
    assert!(old.contains(&"张三@公司.中国".to_string()));
}

#[tokio::test]
async fn test_claude_credentials_precedence_dotted_over_undotted() {
    let temp = tempfile::tempdir().unwrap();
    let dotted_path = temp.path().join(".credentials.json");
    let undotted_path = temp.path().join("credentials.json");

    // In .credentials.json: expired token (timestamp 1000) -> triggers FetchError::AuthExpired
    std::fs::write(
        &dotted_path,
        r#"{"claudeAiOauth": {"accessToken": "tok_dotted", "expiresAt": 1000}}"#,
    ).unwrap();

    // In credentials.json: valid unexpired token (far future)
    std::fs::write(
        &undotted_path,
        r#"{"claudeAiOauth": {"accessToken": "tok_undotted", "expiresAt": 9999999999999}}"#,
    ).unwrap();

    let mut env = std::collections::HashMap::new();
    env.insert("CLAUDE_CONFIG_DIR".into(), temp.path().to_str().unwrap().into());
    let ctx = Ctx::with(
        temp.path().to_path_buf(),
        env,
        std::collections::HashMap::new(),
        std::ffi::OsString::new(),
    );

    // Because .credentials.json takes precedence, it must evaluate .credentials.json first
    // and fail with AuthExpired instead of continuing to credentials.json.
    let err = claude::fetch(&ctx).await.unwrap_err();
    assert!(
        matches!(err, FetchError::AuthExpired),
        "Expected AuthExpired from .credentials.json, got: {:?}",
        err
    );
}

#[tokio::test]
async fn test_claude_credentials_fallback_to_undotted_when_dotted_missing() {
    let temp = tempfile::tempdir().unwrap();
    let undotted_path = temp.path().join("credentials.json");

    // Only credentials.json exists, with expired token
    std::fs::write(
        &undotted_path,
        r#"{"claudeAiOauth": {"accessToken": "tok_undotted", "expiresAt": 1000}}"#,
    ).unwrap();

    let mut env = std::collections::HashMap::new();
    env.insert("CLAUDE_CONFIG_DIR".into(), temp.path().to_str().unwrap().into());
    let ctx = Ctx::with(
        temp.path().to_path_buf(),
        env,
        std::collections::HashMap::new(),
        std::ffi::OsString::new(),
    );

    // .credentials.json is missing, so it must fall back to credentials.json and encounter AuthExpired
    let err = claude::fetch(&ctx).await.unwrap_err();
    assert!(
        matches!(err, FetchError::AuthExpired),
        "Expected fallback to credentials.json resulting in AuthExpired, got: {:?}",
        err
    );
}
