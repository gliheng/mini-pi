use axum::{
    body::Body,
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::sync::Arc;
use subtle::ConstantTimeEq;

use crate::remote::server::AppState;

/// Axum middleware that enforces the optional bearer token.
///
/// The token may be provided either via the `Authorization: Bearer <token>` header
/// or the `access_token` query parameter (needed for EventSource/SSE from browsers,
/// which cannot set custom headers). If no token is configured, every request is allowed.
pub async fn require_bearer_token(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
    next: Next,
) -> Response {
    if let Some(ref expected) = state.bearer_token {
        let provided = extract_token(&req).unwrap_or("");
        let expected_bytes = expected.as_bytes();
        let provided_bytes = provided.as_bytes();

        if !bool::from(expected_bytes.ct_eq(provided_bytes)) {
            return unauthorized_response();
        }
    }

    next.run(req).await
}

fn unauthorized_response() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, "Bearer")],
        Json(json!({ "error": "unauthorized" })),
    )
        .into_response()
}

/// Extract a bearer token from the `Authorization` header or the query string.
fn extract_token(req: &Request<Body>) -> Option<&str> {
    let header_token = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|s| {
            s.get(..7)
                .filter(|p| p.eq_ignore_ascii_case("bearer "))
                .map(|_| &s[7..])
        });

    let query_token = req
        .uri()
        .query()
        .and_then(|query| {
            query.split('&').find_map(|pair| {
                let (k, v) = pair.split_once('=')?;
                if k == "access_token" || k == "token" {
                    Some(v)
                } else {
                    None
                }
            })
        });

    header_token.or(query_token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;

    fn make_request(auth: Option<&str>, path: &str) -> Request<Body> {
        let mut builder = Request::get(path);
        if let Some(value) = auth {
            builder = builder.header(header::AUTHORIZATION, value);
        }
        builder.body(Body::empty()).unwrap()
    }

    #[test]
    fn correct_header_token_extracted() {
        let req = make_request(Some("Bearer secret"), "/status");
        assert_eq!(extract_token(&req), Some("secret"));
    }

    #[test]
    fn wrong_header_token_extracted() {
        let req = make_request(Some("Bearer wrong"), "/status");
        assert_eq!(extract_token(&req), Some("wrong"));
    }

    #[test]
    fn missing_token_returns_none() {
        let req = make_request(None, "/status");
        assert_eq!(extract_token(&req), None);
    }

    #[test]
    fn lowercase_bearer_scheme_allowed() {
        let req = make_request(Some("bearer secret"), "/status");
        assert_eq!(extract_token(&req), Some("secret"));
    }

    #[test]
    fn uppercase_bearer_scheme_allowed() {
        let req = make_request(Some("BEARER secret"), "/status");
        assert_eq!(extract_token(&req), Some("secret"));
    }

    #[test]
    fn query_param_token_allowed() {
        let req = make_request(None, "/threads/1/stream?access_token=secret");
        assert_eq!(extract_token(&req), Some("secret"));
    }

    #[test]
    fn query_param_token_rejected_when_wrong() {
        let req = make_request(None, "/threads/1/stream?access_token=wrong");
        assert_eq!(extract_token(&req), Some("wrong"));
    }

    #[test]
    fn constant_time_comparison_works() {
        assert!(bool::from("secret".as_bytes().ct_eq("secret".as_bytes())));
        assert!(!bool::from("secret".as_bytes().ct_eq("wrong".as_bytes())));
    }
}
