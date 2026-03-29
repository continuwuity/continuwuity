//! `POST /_matrix/policy/unstable/org.matrix.msc4284/sign`
//!
//! Asks a policy server to sign our event

pub mod unstable {
    //! `/policy/unstable/org.matrix.msc4284` ([spec])
    //!
    //! [spec]: https://github.com/matrix-org/matrix-spec-proposals/pull/4284
    use ruma::{
        ServerSignatures,
        api::{federation::authentication::ServerSignatures as ServerSignaturesAuth, request, response}, metadata
    };
    use serde_json::value::RawValue as RawJsonValue;

    metadata! {
        method: POST,
        rate_limited: false,
        authentication: ServerSignaturesAuth,
        history: {
            unstable => "/_matrix/policy/unstable/org.matrix.msc4284/sign",
        }
    }

    /// Response type for the `sign` endpoint.
    #[response]
    pub struct Response {
        /// The signatures returned from the policy server (if provided)
        #[ruma_api(body)]
        pub signatures: Option<ServerSignatures>
    }

    impl Response {
        /// Creates a new `Response` with the given recommendation.
        #[must_use]
        pub fn new(signatures: Option<ServerSignatures>) -> Self {
            Self { signatures }
        }
    }

    /// Request type for the `sign` endpoint.
    #[request]
    pub struct Request {
        /// The PDU body (in canonical JSON)
        #[ruma_api(body)]
        pub pdu: Box<RawJsonValue>,
    }

    impl Request {
        /// Creates a new `Request` with the given event JSON
        #[must_use]
        pub fn new(pdu: Box<RawJsonValue>) -> Self {
            Self { pdu }
        }
    }
}