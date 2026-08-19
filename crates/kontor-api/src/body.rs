//! The request-body extractor, and the one place a malformed body is classified.
//!
//! Every route that takes a body takes it through [`Json`] rather than
//! [`axum::Json`]. The difference is only visible when the body is *wrong*: the
//! stock extractor answers a rejection with `text/plain`, which is not this
//! API's error envelope, so a caller reading the answer sees "something that is
//! not a Kontor Realm" where the truth is "your body did not match the schema".
//! One wrong field then looks exactly like a dead daemon.
//!
//! Classification here is deliberately coarse. The rejection's own message names
//! the offending field *and echoes the offending value*, and
//! [`crate::error`] promises the wire never carries a fragment of the request —
//! a credential pasted into the wrong field would otherwise land in every log
//! that recorded the refusal. So the caller is told which *kind* of malformation
//! happened and is left to read the schema for the rest. Making that schema
//! worth reading is the tool registry's job, not this module's.

use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRequest, Request};
use axum::response::{IntoResponse, Response};
use serde::Serialize;

use crate::error::{ApiError, ApiErrorCode};
use crate::state::ApiState;

/// A JSON body, in or out.
///
/// It stands in for [`axum::Json`] on both sides of a handler so that a route
/// cannot accidentally accept a body through the stock extractor: there is one
/// `Json` in scope in the handler modules, and it is this one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Json<T>(pub T);

impl<T> FromRequest<ApiState> for Json<T>
where
    T: serde::de::DeserializeOwned,
{
    type Rejection = ApiError;

    async fn from_request(request: Request, state: &ApiState) -> Result<Self, Self::Rejection> {
        match axum::Json::<T>::from_request(request, state).await {
            Ok(axum::Json(value)) => Ok(Self(value)),
            Err(rejection) => Err(state.refuse(ApiErrorCode::InvalidRequest, rule(&rejection))),
        }
    }
}

impl<T: Serialize> IntoResponse for Json<T> {
    fn into_response(self) -> Response {
        axum::Json(self.0).into_response()
    }
}

/// The static rule one rejection is reported with.
///
/// Every arm is a fact about the *shape* of what arrived. None of them quotes
/// it.
fn rule(rejection: &JsonRejection) -> &'static str {
    match rejection {
        JsonRejection::JsonDataError(_) => {
            "the request body is valid JSON but does not match this route's schema"
        }
        JsonRejection::JsonSyntaxError(_) => "the request body is not syntactically valid JSON",
        JsonRejection::MissingJsonContentType(_) => {
            "the request body must be sent as content-type: application/json"
        }
        JsonRejection::BytesRejection(_) => "the request body could not be read",
        // `JsonRejection` is `#[non_exhaustive]`. A variant added upstream is
        // still a malformed body, and saying so is honest; guessing which kind
        // would not be.
        _ => "the request body could not be read as this route's schema",
    }
}

// The rejection variants this classifies are `#[non_exhaustive]` with private
// constructors, so there is nothing honest to unit-test here: a hand-built
// `JsonRejection` is not the value axum produces. The behaviour is proved
// end-to-end over the real router instead — see the malformed-body tests in
// `kontor-daemon/tests/loopback_api.rs`.
