use futures::StreamExt;
use num_traits::ToPrimitive;

use crate::Err;

/// Reads the response body while enforcing a maximum size limit to prevent
/// memory exhaustion.
pub async fn limit_read(response: reqwest::Response, max_size: u64) -> crate::Result<Vec<u8>> {
	if response.content_length().is_some_and(|len| len > max_size) {
		return Err!(BadServerResponse("Response too large"));
	}
	let mut data = Vec::new();
	let mut reader = response.bytes_stream();

	while let Some(chunk) = reader.next().await {
		let chunk = chunk?;
		data.extend_from_slice(&chunk);

		if data.len() > max_size.to_usize().expect("max_size must fit in usize") {
			return Err!(BadServerResponse("Response too large"));
		}
	}

	Ok(data)
}

/// Reads the response body as text while enforcing a maximum size limit to
/// prevent memory exhaustion.
pub async fn limit_read_text(
	response: reqwest::Response,
	max_size: u64,
) -> crate::Result<String> {
	let text = String::from_utf8(limit_read(response, max_size).await?)?;
	Ok(text)
}

#[allow(async_fn_in_trait)]
pub trait LimitReadExt {
	async fn limit_read(self, max_size: u64) -> crate::Result<Vec<u8>>;
	async fn limit_read_text(self, max_size: u64) -> crate::Result<String>;
}

impl LimitReadExt for reqwest::Response {
	async fn limit_read(self, max_size: u64) -> crate::Result<Vec<u8>> {
		limit_read(self, max_size).await
	}

	async fn limit_read_text(self, max_size: u64) -> crate::Result<String> {
		limit_read_text(self, max_size).await
	}
}
