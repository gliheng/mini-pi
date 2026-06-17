use subtle::ConstantTimeEq;
use tiny_http::{Header, Request, Response, StatusCode};

/// Check the request's bearer token against the configured token.
/// The token may be provided either via the `Authorization: Bearer <token>` header
/// or the `access_token` query parameter (needed for EventSource/SSE from browsers,
/// which cannot set custom headers).
/// If the token is missing or mismatched, respond immediately with 401.
/// If no token is configured, every request is allowed.
pub fn require_bearer_token<'a>(
    request: &'a Request,
    expected: &Option<String>,
) -> Result<(), Response<std::io::Empty>> {
    let Some(expected) = expected else {
        return Ok(());
    };

    let header_token = request
        .headers()
        .iter()
        .find(|h| h.field.equiv("authorization"))
        .and_then(|h| {
            let s = h.value.as_str();
            s.get(..7)
                .filter(|p| p.eq_ignore_ascii_case("bearer "))
                .map(|_| &s[7..])
        });

    let query_token = request
        .url()
        .split_once('?')
        .and_then(|(_, query)| {
            query.split('&').find_map(|pair| {
                let (k, v) = pair.split_once('=')?;
                if k == "access_token" || k == "token" {
                    Some(v)
                } else {
                    None
                }
            })
        });

    let provided = header_token.or(query_token).unwrap_or("");
    let expected_bytes = expected.as_bytes();
    let provided_bytes = provided.as_bytes();

    if expected_bytes.ct_eq(provided_bytes).into() {
        Ok(())
    } else {
        let response = Response::empty(StatusCode(401))
            .with_header(Header::from_bytes(&b"WWW-Authenticate"[..], &b"Bearer"[..]).unwrap());
        Err(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_request(auth: Option<&str>) -> Request {
        let mut req = tiny_http::TestRequest::new()
            .with_method("GET".parse().unwrap())
            .with_path("/status");
        if let Some(value) = auth {
            req = req.with_header(Header::from_bytes(&b"Authorization"[..], value.as_bytes()).unwrap());
        }
        req.into()
    }

    #[test]
    fn no_token_configured_allows_all() {
        let req = make_request(Some("Bearer secret"));
        assert!(require_bearer_token(&req, &None).is_ok());
    }

    #[test]
    fn correct_token_allowed() {
        let req = make_request(Some("Bearer secret"));
        assert!(require_bearer_token(&req, &Some("secret".to_string())).is_ok());
    }

    #[test]
    fn wrong_token_rejected() {
        let req = make_request(Some("Bearer wrong"));
        assert!(require_bearer_token(&req, &Some("secret".to_string())).is_err());
    }

    #[test]
    fn missing_token_rejected() {
        let req = make_request(None);
        assert!(require_bearer_token(&req, &Some("secret".to_string())).is_err());
    }

    #[test]
    fn lowercase_bearer_scheme_allowed() {
        let req = make_request(Some("bearer secret"));
        assert!(require_bearer_token(&req, &Some("secret".to_string())).is_ok());
    }

    #[test]
    fn uppercase_bearer_scheme_allowed() {
        let req = make_request(Some("BEARER secret"));
        assert!(require_bearer_token(&req, &Some("secret".to_string())).is_ok());
    }

    #[test]
    fn query_param_token_allowed() {
        let req = tiny_http::TestRequest::new()
            .with_method("GET".parse().unwrap())
            .with_path("/threads/1/stream?access_token=secret")
            .into();
        assert!(require_bearer_token(&req, &Some("secret".to_string())).is_ok());
    }

    #[test]
    fn query_param_token_rejected_when_wrong() {
        let req = tiny_http::TestRequest::new()
            .with_method("GET".parse().unwrap())
            .with_path("/threads/1/stream?access_token=wrong")
            .into();
        assert!(require_bearer_token(&req, &Some("secret".to_string())).is_err());
    }
}
