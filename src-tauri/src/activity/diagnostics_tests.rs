use super::reason;

#[test]
fn keeps_actionable_domain_reasons() {
    for input in [
        "resource token expired",
        "authorization revoked",
        "API unavailable",
        "Device is offline; retry after reconnect.",
    ] {
        assert_eq!(reason(input), input);
    }
}

#[test]
fn unstructured_password_rejection_retains_the_cause_without_the_password() {
    assert_eq!(reason("Password wifi-password was rejected"),"Password [redacted] was rejected");
    assert_eq!(reason("Password authentication failed; resource token expired"),"Password authentication failed; resource token expired");
}

#[test]
fn redacts_multiple_assignments_and_json_escaped_values() {
    let input = r#"Request failed: password="pw_SENTINEL", ssid: 'wifi_SENTINEL'; {"api_key":"key\"_SENTINEL", "cookie": "cookie_SENTINEL"}; API unavailable."#;
    let result = reason(input);
    for secret in [
        "pw_SENTINEL",
        "wifi_SENTINEL",
        "key\\\"_SENTINEL",
        "cookie_SENTINEL",
    ] {
        assert!(!result.contains(secret), "{result}");
    }
    assert!(result.contains("Request failed"));
    assert!(result.contains("API unavailable"));
    assert!(result.contains("[redacted]"));
}

#[test]
fn redacts_spaced_api_key_and_unquoted_values_with_spaces() {
    let result = reason("API key: api_SENTINEL value_SENTINEL; API unavailable.");
    assert!(!result.contains("api_SENTINEL"));
    assert!(!result.contains("value_SENTINEL"));
    assert!(result.contains("API unavailable."));
}

#[test]
fn redacts_bearer_and_known_bare_mcp_tokens() {
    let input = "authorization revoked; header Bearer bearer_SENTINEL; generated osg_mcp_secret_SENTINEL; resource token expired";
    let result = reason(input);
    assert!(result.contains("authorization revoked"));
    assert!(result.contains("resource token expired"));
    assert!(!result.contains("bearer_SENTINEL"));
    assert!(!result.contains("osg_mcp_secret_SENTINEL"));
}

#[test]
fn redacts_real_mcp_tokens_and_quoted_authorization_headers() {
    let secret="abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
    let input=format!("generated osg_mcp_6f5902ac-2370-4bbd-9651-6e45f038ce7a.{secret}; API unavailable");
    let result=reason(&input);assert!(!result.contains(secret));assert!(result.contains("API unavailable"));
    let result=reason(r#"Request failed: {"authorization":"Basic YWxpY2U6cHc="}; authorization revoked"#);
    assert!(!result.contains("YWxpY2U6cHc="));assert!(result.contains("authorization revoked"));
}

#[test]
fn redacts_nested_uri_userinfo_and_does_not_drop_surrounding_cause() {
    let input = "Connection failed for https://alice:uri_SENTINEL@host.test/path: API unavailable.";
    let result = reason(input);
    assert!(!result.contains("alice"));
    assert!(!result.contains("uri_SENTINEL"));
    assert!(result.contains("Connection failed"));
    assert!(result.contains("API unavailable"));
}

#[test]
fn unsafe_unstructured_blobs_are_replaced_and_output_is_bounded() {
    assert_eq!(reason("A-REALLY-LONG-SENTINEL-BLOB-01234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890"),"Sensitive diagnostic content was omitted.");
    let result = reason(&format!("{}\u{1}{}", "useful ", "x".repeat(2048)));
    assert!(result.len() <= 1024);
    assert!(!result.chars().any(char::is_control));
}
