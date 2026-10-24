use std::net::{IpAddr, SocketAddr};

use actix_governor::governor::middleware::NoOpMiddleware;
use actix_governor::{
    Governor, GovernorConfig, GovernorConfigBuilder, KeyExtractor, SimpleKeyExtractionError,
};
use actix_web::dev::ServiceRequest;
use actix_web::http::Method;
use actix_web::middleware::Condition;

use crate::error::EmailFormError;
use crate::settings::RateLimitSettings;

#[derive(Debug, Clone)]
pub struct ClientIp {
    trust_proxy_headers: bool,
}

fn parse_addr(value: &str) -> Option<IpAddr> {
    value
        .parse::<IpAddr>()
        .ok()
        .or_else(|| value.parse::<SocketAddr>().ok().map(|addr| addr.ip()))
}

impl KeyExtractor for ClientIp {
    type Key = IpAddr;
    type KeyExtractionError = SimpleKeyExtractionError<&'static str>;

    fn name(&self) -> &'static str {
        "client ip"
    }

    fn extract(&self, req: &ServiceRequest) -> Result<Self::Key, Self::KeyExtractionError> {
        let address = if self.trust_proxy_headers {
            req.connection_info()
                .realip_remote_addr()
                .and_then(parse_addr)
        } else {
            req.peer_addr().map(|addr| addr.ip())
        };

        address.ok_or_else(|| SimpleKeyExtractionError::new("could not determine client address"))
    }

    fn key_name(&self, key: &Self::Key) -> Option<String> {
        Some(key.to_string())
    }
}

#[derive(Clone)]
pub struct RateLimit {
    config: GovernorConfig<ClientIp, NoOpMiddleware>,
    enabled: bool,
}

impl RateLimit {
    pub fn parse(settings: Option<&RateLimitSettings>) -> Result<Self, EmailFormError> {
        let effective = settings.cloned().unwrap_or_default();
        let mut builder = GovernorConfigBuilder::default().key_extractor(ClientIp {
            trust_proxy_headers: effective.trust_proxy_headers,
        });

        builder
            .methods(vec![Method::POST])
            .burst_size(effective.burst_size)
            .seconds_per_request(effective.replenish_seconds);

        let config = builder.finish().ok_or_else(|| {
            config::ConfigError::Message(
                "rate_limit.burst_size and rate_limit.replenish_seconds must be greater than zero"
                    .to_string(),
            )
        })?;

        Ok(Self {
            config,
            enabled: settings.is_some(),
        })
    }

    pub fn middleware(&self) -> Condition<Governor<ClientIp, NoOpMiddleware>> {
        Condition::new(self.enabled, Governor::new(&self.config))
    }
}

#[cfg(test)]
mod tests {
    use actix_web::http::StatusCode;
    use actix_web::{App, HttpResponse, test, web};
    use googletest::prelude::*;

    use super::*;

    fn settings(burst_size: u32, trust_proxy_headers: bool) -> RateLimitSettings {
        RateLimitSettings {
            burst_size,
            replenish_seconds: 3600,
            trust_proxy_headers,
        }
    }

    async fn statuses(
        settings: Option<RateLimitSettings>,
        requests: Vec<test::TestRequest>,
    ) -> Vec<StatusCode> {
        let rate_limit = RateLimit::parse(settings.as_ref()).unwrap();
        let app = test::init_service(
            App::new()
                .wrap(rate_limit.middleware())
                .route("/submit", web::post().to(HttpResponse::Ok))
                .route("/submit", web::get().to(HttpResponse::Ok)),
        )
        .await;
        let mut statuses = Vec::new();

        for request in requests {
            let response = test::call_service(&app, request.to_request()).await;

            statuses.push(response.status());
        }

        statuses
    }

    fn post_from(peer: &str) -> test::TestRequest {
        test::TestRequest::post()
            .uri("/submit")
            .peer_addr(peer.parse().unwrap())
    }

    #[actix_web::test]
    async fn rejects_requests_over_the_burst_size() {
        let requests = (0..4).map(|_| post_from("10.0.0.1:1000")).collect();

        let result = statuses(Some(settings(2, false)), requests).await;

        assert_that!(
            result,
            elements_are![
                eq(&StatusCode::OK),
                eq(&StatusCode::OK),
                eq(&StatusCode::TOO_MANY_REQUESTS),
                eq(&StatusCode::TOO_MANY_REQUESTS),
            ]
        );
    }

    #[actix_web::test]
    async fn tracks_each_client_address_separately() {
        let requests = vec![
            post_from("10.0.0.1:1000"),
            post_from("10.0.0.2:1000"),
            post_from("10.0.0.1:2000"),
            post_from("10.0.0.2:2000"),
        ];

        let result = statuses(Some(settings(1, false)), requests).await;

        assert_that!(
            result,
            elements_are![
                eq(&StatusCode::OK),
                eq(&StatusCode::OK),
                eq(&StatusCode::TOO_MANY_REQUESTS),
                eq(&StatusCode::TOO_MANY_REQUESTS),
            ]
        );
    }

    #[actix_web::test]
    async fn does_not_limit_when_disabled() {
        let requests = (0..20).map(|_| post_from("10.0.0.1:1000")).collect();

        let result = statuses(None, requests).await;

        assert_that!(result, each(eq(&StatusCode::OK)));
    }

    #[actix_web::test]
    async fn does_not_limit_other_methods() {
        let requests = (0..5)
            .map(|_| {
                test::TestRequest::get()
                    .uri("/submit")
                    .peer_addr("10.0.0.1:1000".parse().unwrap())
            })
            .collect();

        let result = statuses(Some(settings(1, false)), requests).await;

        assert_that!(result, each(eq(&StatusCode::OK)));
    }

    #[actix_web::test]
    async fn ignores_forwarded_header_unless_trusted() {
        let requests = vec![
            post_from("10.0.0.1:1000").insert_header(("X-Forwarded-For", "203.0.113.1")),
            post_from("10.0.0.1:1000").insert_header(("X-Forwarded-For", "203.0.113.2")),
        ];

        let result = statuses(Some(settings(1, false)), requests).await;

        assert_that!(
            result,
            elements_are![eq(&StatusCode::OK), eq(&StatusCode::TOO_MANY_REQUESTS)]
        );
    }

    #[actix_web::test]
    async fn uses_forwarded_header_when_trusted() {
        let requests = vec![
            post_from("10.0.0.1:1000").insert_header(("X-Forwarded-For", "203.0.113.1")),
            post_from("10.0.0.1:1000").insert_header(("X-Forwarded-For", "203.0.113.2")),
            post_from("10.0.0.1:1000").insert_header(("X-Forwarded-For", "203.0.113.1")),
        ];

        let result = statuses(Some(settings(1, true)), requests).await;

        assert_that!(
            result,
            elements_are![
                eq(&StatusCode::OK),
                eq(&StatusCode::OK),
                eq(&StatusCode::TOO_MANY_REQUESTS),
            ]
        );
    }

    #[actix_web::test]
    async fn falls_back_to_peer_address_when_trusted_header_is_absent() {
        let requests = vec![post_from("10.0.0.1:1000"), post_from("10.0.0.1:2000")];

        let result = statuses(Some(settings(1, true)), requests).await;

        assert_that!(
            result,
            elements_are![eq(&StatusCode::OK), eq(&StatusCode::TOO_MANY_REQUESTS)]
        );
    }

    #[actix_web::test]
    async fn rejects_zero_values() {
        let zero_burst = RateLimitSettings {
            burst_size: 0,
            ..settings(1, false)
        };
        let zero_period = RateLimitSettings {
            replenish_seconds: 0,
            ..settings(1, false)
        };

        for invalid in [zero_burst, zero_period] {
            let result = RateLimit::parse(Some(&invalid)).map(|_| ());

            assert_that!(result, err(matches_pattern!(EmailFormError::Config(_))));
        }
    }

    #[actix_web::test]
    async fn parses_ip_with_and_without_port() {
        assert_that!(
            parse_addr("203.0.113.1"),
            some(eq("203.0.113.1".parse::<IpAddr>().unwrap()))
        );
        assert_that!(
            parse_addr("203.0.113.1:4000"),
            some(eq("203.0.113.1".parse::<IpAddr>().unwrap()))
        );
        assert_that!(
            parse_addr("[::1]:4000"),
            some(eq("::1".parse::<IpAddr>().unwrap()))
        );
        assert_that!(parse_addr("not an address"), none());
    }
}
