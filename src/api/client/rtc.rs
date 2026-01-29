use axum::{Json, extract::State};
use conduwuit::Result;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::{Deserialize, Serialize};
use ruma::api::client::account::whoami;

use crate::Ruma;

#[derive(Debug, Serialize, Deserialize)]
struct LiveKitClaims {
	sub: String,
	iss: String,
	exp: usize,
	video: VideoGrant,
    name: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct VideoGrant {
	roomCreate: bool,
	roomList: bool,
	roomJoin: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Transport {
    #[serde(rename = "type")]
    pub type_: String,
    pub params: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct RtcTransportsResponse {
    pub transports: Vec<Transport>,
}


/// # `GET /_matrix/client/unstable/org.matrix.msc4143/rtc/transports`
///
/// Returns a list of available RTC transports.
pub(crate) async fn get_rtc_transports_route(
	State(services): State<crate::State>,
    // We use `whoami` request because it requires authentication and has an empty body,
    // which matches the signature we want for this GET endpoint while ensuring `Ruma` handles auth.
	body: Ruma<whoami::v3::Request>,
) -> Result<Json<RtcTransportsResponse>> {
    let mut transports = Vec::new();

    if let (Some(url), Some(secret), Some(key)) = (
        &services.server.config.livekit_url,
        &services.server.config.livekit_secret,
        &services.server.config.livekit_key,
    ) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();

        // `body` (Ruma wrapper) contains the authenticated `sender_user`.
        let sender_user = body.sender_user.as_ref().ok_or_else(|| {
             conduwuit::Error::BadRequest(conduwuit::ErrorKind::MissingToken, "Missing access token")
        })?;

        let claims = LiveKitClaims {
            sub: sender_user.to_string(),
            iss: key.clone(),
            exp: (now + 3600) as usize, // Token valid for 1 hour
            video: VideoGrant {
                roomCreate: true,
                roomList: true,
                roomJoin: true,
            },
            name: sender_user.to_string(),
        };

        let token = encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        ).map_err(|e| {
            conduwuit::Error::internal(format!("Failed to generate LiveKit token: {}", e))
        })?;

        let mut params = std::collections::BTreeMap::new();
        params.insert("url".to_string(), url.clone());
        params.insert("token".to_string(), token);

        transports.push(Transport {
            type_: "org.matrix.msc4143.v1.livekit".to_string(),
            params,
        });
    }

	Ok(Json(RtcTransportsResponse {
		transports,
	}))
}
