//! URL Previews
//!
//! This functionality is gated by 'url_preview', but not at the unit level for
//! historical and simplicity reasons. Instead the feature gates the inclusion
//! of dependencies and nulls out results through the existing interface when
//! not featured.

use std::time::SystemTime;

use conduwuit::{Err, Result, debug, err, utils::response::LimitReadExt, info};
use ipaddress::IPAddress;
use lettre::message::header::ContentType;
use ruma::OwnedMxcUri;
use serde::Serialize;
use url::Url;

use super::Service;
use crate::media::mxc::Mxc;

#[derive(Serialize, Default, Debug)]
pub struct UrlPreviewData {
	// From https://ogp.me/
	/// The title of your object as it should appear within the graph
	/// "The Rock".
	#[serde(skip_serializing_if = "Option::is_none", rename(serialize = "og:title"))]
	pub title: Option<String>,

	/// The type of your object, e.g., "video.movie".
	#[serde(skip_serializing_if = "String::is_empty", rename(serialize = "og:type"))]
	pub site_type: String,
	/// The canonical URL of your object that will be used as its permanent ID
	/// in the graph.
	#[serde(skip_serializing_if = "Option::is_none", rename(serialize = "og:url"))]
	pub url: Option<String>,
	/// A one to two sentence description of your object.
	#[serde(skip_serializing_if = "Option::is_none", rename(serialize = "og:description"))]
	pub description: Option<String>,
	/// The word that appears before this object's title in a sentence.
	#[serde(skip_serializing_if = "Option::is_none", rename(serialize = "og:determiner"))]
	pub determiner: Option<String>,
	/// The locale these tags are marked up in
	#[serde(skip_serializing_if = "Option::is_none", rename(serialize = "og:locale"))]
	pub locale: Option<String>,
	/// If your object is part of a larger web site, the name which should be
	/// displayed for the overall site.
	#[serde(skip_serializing_if = "Option::is_none", rename(serialize = "og:site_name"))]
	pub site_name: Option<String>,

	/// An image URL which should represent your object within the graph.
	#[serde(skip_serializing_if = "Option::is_none", rename(serialize = "og:image"))]
	pub image: Option<String>,
	/// A MIME type for this image.
	#[serde(skip_serializing_if = "Option::is_none", rename(serialize = "og:image:type"))]
	pub image_type: Option<ContentType>,
	#[serde(skip_serializing_if = "Option::is_none", rename(serialize = "matrix:image:size"))]
	pub image_size: Option<usize>,
	/// The number of pixels wide.
	#[serde(skip_serializing_if = "Option::is_none", rename(serialize = "og:image:width"))]
	pub image_width: Option<u32>,
	/// The number of pixels high.
	#[serde(skip_serializing_if = "Option::is_none", rename(serialize = "og:image:height"))]
	pub image_height: Option<u32>,
	/// A description of what is in the image (not a caption).
	#[serde(skip_serializing_if = "Option::is_none", rename(serialize = "og:image:alt"))]
	pub image_alt: Option<String>,

	/// A URL to a video file that complements this object.
	#[serde(skip_serializing_if = "Option::is_none", rename(serialize = "og:video"))]
	pub video: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none", rename(serialize = "matrix:video:size"))]
	pub video_size: Option<usize>,
	#[serde(skip_serializing_if = "Option::is_none", rename(serialize = "og:video:width"))]
	pub video_width: Option<u32>,
	#[serde(skip_serializing_if = "Option::is_none", rename(serialize = "og:video:height"))]
	pub video_height: Option<u32>,
	#[serde(skip_serializing_if = "Option::is_none", rename(serialize = "og:video:type"))]
	pub video_type: Option<ContentType>,
	#[serde(skip_serializing_if = "Option::is_none", rename(serialize = "og:video:alt"))]
	pub video_alt: Option<String>,
	/// A URL to an audio file to accompany this object.
	#[serde(skip_serializing_if = "Option::is_none", rename(serialize = "og:audio"))]
	pub audio: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none", rename(serialize = "matrix:audio:size"))]
	pub audio_size: Option<usize>,
}

impl Service {
	pub fn remove_url_preview(&self, url: &str) -> impl Future<Output = Result<()>> {
		// TODO: also remove the downloaded image
		std::future::ready(self.db.remove_url_preview(url))
	}

	pub async fn clear_url_previews(&self) { self.db.clear_url_previews().await; }

	pub fn set_url_preview(
		&self,
		url: &str,
		data: &UrlPreviewData,
	) -> impl Future<Output = Result<()>> {
		let now = SystemTime::now()
			.duration_since(SystemTime::UNIX_EPOCH)
			.expect("valid system time");
		std::future::ready(self.db.set_url_preview(url, data, now))
	}

	pub async fn get_url_preview(&self, url: &Url) -> Result<UrlPreviewData> {
		if let Ok(preview) = self.db.get_url_preview(url.as_str()).await {
			return Ok(preview);
		}

		// ensure that only one request is made per URL
		let _request_lock = self.url_preview_mutex.lock(url.as_str()).await;

		match self.db.get_url_preview(url.as_str()).await {
			| Ok(preview) => Ok(preview),
			| Err(_) => self.request_url_preview(url).await,
		}
	}

	async fn request_url_preview(&self, url: &Url) -> Result<UrlPreviewData> {
		if let Ok(ip) = IPAddress::parse(url.host_str().expect("URL previously validated")) {
			if !self.services.client.valid_cidr_range(&ip) {
				return Err!(Request(Forbidden("Requesting from this address is forbidden")));
			}
		}

		let client = &self.services.client.url_preview;
		let response = client.head(url.as_str()).send().await?;

		debug!(%url, "URL preview response headers: {:?}", response.headers());

		if let Some(remote_addr) = response.remote_addr() {
			debug!(%url, "URL preview response remote address: {:?}", remote_addr);

			if let Ok(ip) = IPAddress::parse(remote_addr.ip().to_string()) {
				if !self.services.client.valid_cidr_range(&ip) {
					return Err!(Request(Forbidden("Requesting from this address is forbidden")));
				}
			}
		}

		let Some(content_type) = response.headers().get(reqwest::header::CONTENT_TYPE) else {
			return Err!(Request(Unknown(
				"Response contains unknown, invalid, or missing Content-Type header"
			)));
		};

		let content_type = content_type
			.to_str()
			.map_err(|e| err!(Request(Unknown("Unknown or invalid Content-Type header: {e}"))))?;

		let data = match content_type {
			| html if html.starts_with("text/html") => self.download_html(url.as_str()).await?,
			| img if img.starts_with("image/") => self.download_image(url.as_str()).await?,
			| video if video.starts_with("video/") => self.download_video(url.as_str()).await?,
			| audio if audio.starts_with("audio/") => self.download_audio(url.as_str()).await?,
			| other => return Err!(Request(Unknown("Unsupported Content-Type: {other}"))),
		};

		self.set_url_preview(url.as_str(), &data).await?;

		Ok(data)
	}

	#[cfg(feature = "url_preview")]
	pub async fn download_image(&self, url: &str) -> Result<UrlPreviewData> {
		use conduwuit::utils::random_string;
		use image::ImageReader;

		use crate::media::mxc::Mxc;

		let mut preview_data = UrlPreviewData::default();

		let response = self.services.client.url_preview.get(url).send().await?;
		if !response.status().is_success() {
			return Err!(Request(Unknown("Image download returned HTTP {}", response.status())));
		}

		let image = response
			.limit_read(
				self.services
					.server
					.config
					.max_request_size
					.try_into()
					.expect("u64 should fit in usize"),
			)
			.await?;

		let reader = ImageReader::new(std::io::Cursor::new(&image)).with_guessed_format()?;
		let content_type = reader
			.format()
			.ok_or_else(|| err!(Request(Unknown("Downloaded file is not a supported image"))))?
			.to_mime_type();

		let (width, height) = reader.into_dimensions().map_err(|error| {
			err!(Request(Unknown("Failed to read preview image dimensions: {error}")))
		})?;
		preview_data.image_width.get_or_insert(width);
		preview_data.image_height.get_or_insert(height);
		preview_data.image_type = content_type.parse().ok();

		let mxc = Mxc {
			server_name: self.services.globals.server_name(),
			media_id: &random_string(super::MXC_LENGTH),
		};

		self.create(&mxc, None, None, Some(content_type), &image)
			.await?;

		preview_data.image = Some(mxc.to_string());

		Ok(preview_data)
	}

	#[cfg(feature = "url_preview")]
	#[tracing::instrument(skip(self))]
	pub async fn download_video(&self, url: &str) -> Result<UrlPreviewData> {
		let mut preview_data = UrlPreviewData::default();

		if self.services.globals.url_preview_allow_audio_video() {
			let (url, size) = self.download_media(url).await?;
			preview_data.video = Some(url.to_string());
			preview_data.video_size = Some(size);
		}

		Ok(preview_data)
	}

	#[cfg(feature = "url_preview")]
	#[tracing::instrument(skip(self))]
	pub async fn download_audio(&self, url: &str) -> Result<UrlPreviewData> {
		let mut preview_data = UrlPreviewData::default();

		if self.services.globals.url_preview_allow_audio_video() {
			let (url, size) = self.download_media(url).await?;
			preview_data.audio = Some(url.to_string());
			preview_data.audio_size = Some(size);
		}

		Ok(preview_data)
	}

	#[tracing::instrument(skip(self))]
	pub async fn download_media(&self, url: &str) -> Result<(OwnedMxcUri, usize)> {
		use conduwuit::utils::random_string;
		use http::header::CONTENT_TYPE;

		let response = self.services.client.url_preview.get(url).send().await?;
		let content_type = response.headers().get(CONTENT_TYPE).cloned();
		let media = response
			.limit_read(
				self.services
					.server
					.config
					.max_request_size
					.try_into()
					.expect("u64 should fit in usize"),
			)
			.await?;

		let mxc = Mxc {
			server_name: self.services.globals.server_name(),
			media_id: &random_string(super::MXC_LENGTH),
		};

		let content_type = content_type.and_then(|v| v.to_str().map(ToOwned::to_owned).ok());
		self.create(&mxc, None, None, content_type.as_deref(), &media)
			.await?;

		Ok((OwnedMxcUri::from(mxc.to_string()), media.len()))
	}

	#[cfg(not(feature = "url_preview"))]
	pub async fn download_image(
		&self,
		_url: &str,
		_preview_data: Option<UrlPreviewData>,
	) -> Result<UrlPreviewData> {
		Err!(FeatureDisabled("url_preview"))
	}

	#[cfg(not(feature = "url_preview"))]
	pub async fn download_video(
		&self,
		_url: &str,
		_preview_data: Option<UrlPreviewData>,
	) -> Result<UrlPreviewData> {
		Err!(FeatureDisabled("url_preview"))
	}

	#[cfg(not(feature = "url_preview"))]
	pub async fn download_audio(
		&self,
		_url: &str,
		_preview_data: Option<UrlPreviewData>,
	) -> Result<UrlPreviewData> {
		Err!(FeatureDisabled("url_preview"))
	}

	#[cfg(feature = "url_preview")]
	#[tracing::instrument(skip(self))]
	async fn download_html(&self, url: &str) -> Result<UrlPreviewData> {
		use webpage::HTML;

		let client = &self.services.client.url_preview;
		let body = client
			.get(url)
			.send()
			.await?
			.limit_read_text(
				self.services
					.server
					.config
					.max_request_size
					.try_into()
					.expect("u64 should fit in usize"),
			)
			.await?;
		let Ok(html) = HTML::from_string(body.clone(), Some(url.to_owned()))
			.inspect_err(|e| debug!(error=?e, "Failed to parse HTML data"))
		else {
			return Err!(Request(Unknown("Failed to parse HTML")));
		};

		let mut preview_data = UrlPreviewData::from(&html.opengraph);
		debug!(parsed=?preview_data, deserialised=?html.opengraph, "Parsed opengraph data");

		if let Some(obj) = html.opengraph.images.first() {
			info!(asset_url=%obj.url, "Downloading preview image");
			if let Ok(image_data) = self.download_image(&obj.url).await.inspect_err(
				|e| info!(error=?e, asset_url=%obj.url, "Could not download URL preview image"),
			) {
				preview_data.image = image_data.image;
				preview_data.image_size = image_data.image_size;
				// Prioritise what the top-level site specified, falling back to
				// detected params.
				preview_data.image_width = preview_data.image_width.or(image_data.image_width);
				preview_data.image_height = preview_data.image_height.or(image_data.image_height);
				preview_data.image_type = preview_data.image_type.or(image_data.image_type);
			}
		}

		if let Some(obj) = html.opengraph.videos.first() {
			info!(asset_url=%obj.url, "Downloading preview video");
			if let Ok(video_data) = self.download_video(&obj.url).await.inspect_err(
				|e| info!(error=?e, asset_url=%obj.url, "Could not download URL preview video"),
			) {
				preview_data.video = video_data.video;
				preview_data.video_size = video_data.video_size;
			}
		}

		if let Some(obj) = html.opengraph.audios.first() {
			info!(asset_url=%obj.url, "Downloading URL preview audio");
			if let Ok(audio_data) = self.download_audio(&obj.url).await.inspect_err(
				|e| info!(error=?e, asset_url=%obj.url, "Could not download URL preview audio"),
			) {
				preview_data.audio = audio_data.audio;
				preview_data.audio_size = audio_data.audio_size;
			}
		}

		// Provide defaults for some params
		preview_data.title = preview_data.title.or(html.title);
		preview_data.description = preview_data.description.or(html.description);
		preview_data.url = preview_data.url.or_else(|| Some(url.to_owned()));

		// We remove og:image:*, og:video:*, and og:audio:* if their respective
		// prior-level properties are not present.
		// We do this here instead of in the From<> impl because we can't know
		// if we'll be able to fetch the media to convert it to an mxc that
		// point.
		if preview_data.image.is_none() {
			preview_data.image_type = None;
			preview_data.image_alt = None;
			preview_data.image_height = None;
			preview_data.image_width = None;
		}
		if preview_data.video.is_none() {
			preview_data.video_type = None;
			preview_data.video_alt = None;
			preview_data.video_height = None;
			preview_data.video_width = None;
		}
		if preview_data.audio.is_none() {
			preview_data.audio_size = None;
		}

		Ok(preview_data)
	}

	#[cfg(not(feature = "url_preview"))]
	async fn download_html(&self, _url: &str) -> Result<UrlPreviewData> {
		Err!(FeatureDisabled("url_preview"))
	}

	pub fn url_preview_allowed(&self, url: &Url) -> bool {
		if ["http", "https"]
			.iter()
			.all(|&scheme| scheme != url.scheme().to_lowercase())
		{
			debug!("Ignoring non-HTTP/HTTPS URL to preview: {}", url);
			return false;
		}

		let host = match url.host_str() {
			| None => {
				debug!("Ignoring URL preview for a URL that does not have a host (?): {}", url);
				return false;
			},
			| Some(h) => h.to_owned(),
		};

		let allowlist_domain_contains = self
			.services
			.globals
			.url_preview_domain_contains_allowlist();
		let allowlist_domain_explicit = self
			.services
			.globals
			.url_preview_domain_explicit_allowlist();
		let denylist_domain_explicit =
			self.services.globals.url_preview_domain_explicit_denylist();
		let allowlist_url_contains = self.services.globals.url_preview_url_contains_allowlist();

		if allowlist_domain_contains.contains(&"*".to_owned())
			|| allowlist_domain_explicit.contains(&"*".to_owned())
			|| allowlist_url_contains.contains(&"*".to_owned())
		{
			debug!(
				"Config key contains * which is allowing all URL previews. Allowing URL {}",
				url
			);
			return true;
		}

		if !host.is_empty() {
			if denylist_domain_explicit.contains(&host) {
				debug!(
					"Host {} is not allowed by url_preview_domain_explicit_denylist (check 1/4)",
					&host
				);
				return false;
			}

			if allowlist_domain_explicit.contains(&host) {
				debug!(
					"Host {} is allowed by url_preview_domain_explicit_allowlist (check 2/4)",
					&host
				);
				return true;
			}

			if allowlist_domain_contains
				.iter()
				.any(|domain_s| domain_s.contains(&host.clone()))
			{
				debug!(
					"Host {} is allowed by url_preview_domain_contains_allowlist (check 3/4)",
					&host
				);
				return true;
			}

			if allowlist_url_contains
				.iter()
				.any(|url_s| url.to_string().contains(url_s))
			{
				debug!(
					"URL {} is allowed by url_preview_url_contains_allowlist (check 4/4)",
					&host
				);
				return true;
			}

			// check root domain if available and if user has root domain checks
			if self.services.globals.url_preview_check_root_domain() {
				debug!("Checking root domain");
				match host.split_once('.') {
					| None => return false,
					| Some((_, root_domain)) => {
						if denylist_domain_explicit.contains(&root_domain.to_owned()) {
							debug!(
								"Root domain {} is not allowed by \
								 url_preview_domain_explicit_denylist (check 1/3)",
								&root_domain
							);
							return false;
						}

						if allowlist_domain_explicit.contains(&root_domain.to_owned()) {
							debug!(
								"Root domain {} is allowed by \
								 url_preview_domain_explicit_allowlist (check 2/3)",
								&root_domain
							);
							return true;
						}

						if allowlist_domain_contains
							.iter()
							.any(|domain_s| domain_s.contains(&root_domain.to_owned()))
						{
							debug!(
								"Root domain {} is allowed by \
								 url_preview_domain_contains_allowlist (check 3/3)",
								&root_domain
							);
							return true;
						}
					},
				}
			}
		}

		false
	}
}

impl From<&webpage::Opengraph> for UrlPreviewData {
	fn from(og: &webpage::Opengraph) -> Self {
		Self {
			title: og.properties.get("title").cloned(),
			description: og.properties.get("description").cloned(),
			site_type: og.og_type.clone(),
			url: og.properties.get("url").cloned(),
			determiner: og.properties.get("determiner").cloned(),
			locale: og.properties.get("locale").cloned(),
			site_name: og.properties.get("site_name").cloned(),
			image_type: og
				.images
				.first()
				.and_then(|img| img.properties.get("type").and_then(|t| t.parse().ok())),
			image_width: og
				.images
				.first()
				.and_then(|img| img.properties.get("width").and_then(|w| w.parse().ok())),
			image_height: og
				.images
				.first()
				.and_then(|img| img.properties.get("height").and_then(|h| h.parse().ok())),
			image_alt: og
				.images
				.first()
				.and_then(|img| img.properties.get("alt").cloned()),
			video_type: og
				.videos
				.first()
				.and_then(|vid| vid.properties.get("type"))
				.and_then(|t| t.parse().ok()),
			video_width: og
				.videos
				.first()
				.and_then(|vid| vid.properties.get("width"))
				.and_then(|w| w.parse().ok()),
			video_height: og
				.videos
				.first()
				.and_then(|vid| vid.properties.get("height"))
				.and_then(|h| h.parse().ok()),
			video_alt: og
				.videos
				.first()
				.and_then(|vid| vid.properties.get("alt").cloned()),
			..Default::default()
		}
	}
}

#[cfg(test)]
mod tests {
	use webpage::HTML;

	use super::UrlPreviewData;

	#[test]
	fn test_urlpreviewdata_parses_correctly() {
		let html = r#"
			<html>
				<head>
					<meta property="og:title" content="Test Title" />
					<meta property="og:description" content="Test Description" />
					<meta property="og:image" content="https://example.com/image.png" />
					<meta property="og:url" content="https://example.com/index.html" />
					<meta property="og:type" content="org.continuwuity.other" />
					<meta property="og:site_name" content="Example Site" />
					<meta property="og:image:alt" content="An example image" />
					<meta property="og:image:type" content="image/png" />
					<meta property="og:image:height" content="69" />
					<meta property="og:image:width" content="420" />
					<meta property="og:video" content="https://example.com/video.mp4" />
					<meta property="og:audio" content="https://example.com/audio.ogg" />
				</head>
				<body></body>
			</html>
		"#;

		let parsed_html =
			HTML::from_string(html.to_owned(), Some("https://example.com".to_owned()))
				.expect("Failed to parse HTML");

		let data = UrlPreviewData::from(&parsed_html.opengraph);

		assert_eq!(data.title, Some("Test Title".to_owned()));
		assert_eq!(data.description, Some("Test Description".to_owned()));
		assert_eq!(
			data.image, None,
			"UrlPreviewData should not inherit HTTP/S URL from URL preview"
		);
		assert_eq!(
			data.video, None,
			"UrlPreviewData should not inherit HTTP/S URL from URL preview"
		);
		assert_eq!(
			data.audio, None,
			"UrlPreviewData should not inherit HTTP/S URL from URL preview"
		);
		assert_eq!(data.url, Some("https://example.com/index.html".to_owned()));
		assert_eq!(data.site_type, "org.continuwuity.other".to_owned());
		assert_eq!(data.site_name, Some("Example Site".to_owned()));
		assert_eq!(data.image_alt, Some("An example image".to_owned()));
		assert_eq!(data.image_type, Some("image/png".parse().unwrap()));
		assert_eq!(data.image_height, Some(69));
		assert_eq!(data.image_width, Some(420));
		assert_eq!(data.image_size, None, "*_size must be None when pulling from opengraph");
		assert_eq!(data.video_size, None, "*_size must be None when pulling from opengraph");
		assert_eq!(data.audio_size, None, "*_size must be None when pulling from opengraph");
	}
}
