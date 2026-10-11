use crate::auth::users::Backend;
use crate::ice::IceError;
use axum::Json;
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum_login::Error as LoginError;
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProblemDetails {
    #[serde(rename = "type")]
    pub type_url: String,
    pub title: String,
    pub status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
    #[serde(skip)]
    extra_headers: Vec<(HeaderName, HeaderValue)>,
}

impl ProblemDetails {
    pub fn new(status: u16, title: &str) -> Self {
        Self {
            type_url: "about:blank".to_string(),
            title: title.to_string(),
            status,
            detail: None,
            instance: None,
            extra_headers: Vec::new(),
        }
    }

    pub fn with_title(mut self, title: &str) -> Self {
        self.title = title.to_string();
        self
    }

    pub fn with_type_url(mut self, type_url: &str) -> Self {
        self.type_url = type_url.to_string();
        self
    }

    pub fn with_detail(mut self, detail: &str) -> Self {
        self.detail = Some(detail.to_string());
        self
    }

    pub fn with_instance(mut self, instance: &str) -> Self {
        self.instance = Some(instance.to_string());
        self
    }

    pub fn with_header(
        mut self,
        name: impl Into<HeaderName>,
        value: impl Into<HeaderValue>,
    ) -> Self {
        self.extra_headers.push((name.into(), value.into()));
        self
    }
}

impl From<StatusCode> for ProblemDetails {
    fn from(status: StatusCode) -> Self {
        Self::new(
            status.as_u16(),
            status.canonical_reason().unwrap_or(status.as_str()),
        )
    }
}

impl IntoResponse for ProblemDetails {
    fn into_response(self) -> Response {
        let mut headers = HeaderMap::with_capacity(self.extra_headers.len() + 1);

        // RFC7807 media type
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/problem+json"),
        );

        for (name, value) in &self.extra_headers {
            headers.insert(name.clone(), value.clone());
        }

        (
            StatusCode::from_u16(self.status).unwrap(),
            headers,
            Json(&self),
        )
            .into_response()
    }
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Bad Request: {0}")]
    BadRequest(String),
    #[error("Unauthorized: {0}")]
    Unauthorized(String),
    #[error("Not Found")]
    NotFound,
    #[error("Too Many Requests: retry after {0}")]
    TooManyRequests(u64),
    #[error(transparent)]
    InternalServerError(#[from] anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            AppError::BadRequest(msg) => {
                tracing::debug!(detail = %msg, "Bad request");
                ProblemDetails::new(StatusCode::BAD_REQUEST.as_u16(), "Bad Request")
                    .with_detail(&msg)
            }
            AppError::Unauthorized(msg) => {
                tracing::debug!(detail = %msg, "Unauthorized");
                ProblemDetails::new(StatusCode::UNAUTHORIZED.as_u16(), "Unauthorized")
                    .with_detail(&msg)
            }
            AppError::NotFound => ProblemDetails::new(StatusCode::NOT_FOUND.as_u16(), "Not Found"),
            AppError::TooManyRequests(retry_after_secs) => {
                tracing::debug!(retry_after_secs, "Rate limit exceeded");
                ProblemDetails::new(StatusCode::TOO_MANY_REQUESTS.as_u16(), "Too Many Requests")
                    .with_detail(&format!("Rate limit for this endpoint was exceeded. Try again in {retry_after_secs} seconds."))
                    .with_header(header::RETRY_AFTER, HeaderValue::from_str(&retry_after_secs.to_string()).expect("retry-after header value is valid"))
            }
            AppError::InternalServerError(err) => {
                tracing::error!(err = %format_args!("{err:#}"), "Internal server error");
                ProblemDetails::new(
                    StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                    "Internal Server Error",
                )
            }
        }
        .into_response()
    }
}

impl From<LoginError<Backend>> for AppError {
    fn from(err: LoginError<Backend>) -> Self {
        match err {
            LoginError::Backend(err) => err,
            LoginError::Session(err) => AppError::InternalServerError(err.into()),
        }
    }
}

impl From<String> for AppError {
    fn from(err: String) -> Self {
        AppError::InternalServerError(anyhow::anyhow!(err))
    }
}

impl From<IceError> for AppError {
    fn from(err: IceError) -> Self {
        AppError::InternalServerError(anyhow::anyhow!(err))
    }
}
