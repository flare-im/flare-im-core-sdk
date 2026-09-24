//! Wire-only presence request DTOs shared with native-target tests.

use serde::Serialize;

/// The API gateway intentionally accepts snake_case for this request body.
#[derive(Debug, Serialize)]
pub(crate) struct BatchGetUserPresenceHttpRequest {
    pub(crate) user_ids: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_request_serializes_gateway_snake_case_user_ids() {
        let payload = BatchGetUserPresenceHttpRequest {
            user_ids: vec!["alice".into(), "bob".into()],
        };
        let json = serde_json::to_value(payload).expect("serialize batch request");
        assert_eq!(
            json.get("user_ids")
                .and_then(|value| value.as_array())
                .map(Vec::len),
            Some(2)
        );
        assert!(json.get("userIds").is_none());
    }
}
