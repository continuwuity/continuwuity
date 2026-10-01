#![cfg(test)]

#[cfg(feature = "url_preview")]
mod url_preview {
	use std::{io::Cursor, sync::Arc};

	use axum::{Router, body::Body, http::StatusCode, routing::get};
	use conduwuit::{
		Config, Server,
		log::{Log, LogLevelReloadHandles},
	};
	use image::{DynamicImage, ImageFormat};
	use serde_json::json;
	use tokio::net::TcpListener;

	use crate::{
		Services,
		media::{mxc::Mxc, preview::UrlPreviewData},
	};

	async fn services() -> (tempfile::TempDir, Arc<Services>) {
		_ = rustls::crypto::ring::default_provider().install_default();
		let directory = tempfile::tempdir().unwrap();
		let config: Config = serde_json::from_value(json!({
			"server_name": "example.com",
			"database_path": directory.path(),
			"db_pool_affinity": false,
			"db_pool_workers": 1,
			"rocksdb_parallelism_threads": 1,
			"db_cache_capacity_mb": 8,
			"db_write_buffer_capacity_mb": 8,
			"max_request_size": 4096,
		}))
		.unwrap();
		let server =
			Arc::new(Server::new(config, Some(tokio::runtime::Handle::current()), Log {
				reload: LogLevelReloadHandles::default(),
				capture: Arc::default(),
			}));
		let services = Services::build(server).await.unwrap();
		services.media.create_media_dir().await.unwrap();
		(directory, services)
	}

	fn image(format: ImageFormat) -> Vec<u8> {
		let mut bytes = Cursor::new(Vec::new());
		DynamicImage::new_rgb8(2, 3)
			.write_to(&mut bytes, format)
			.unwrap();
		bytes.into_inner()
	}

	struct ImageServer {
		url: String,
		task: tokio::task::JoinHandle<()>,
	}

	impl ImageServer {
		async fn new(
			services: &Services,
			status: StatusCode,
			content_type: Option<&str>,
			body: Vec<u8>,
		) -> Self {
			let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
			let url = format!("http://{}/image", listener.local_addr().unwrap());
			let mut response = http::Response::builder().status(status);
			if let Some(content_type) = content_type {
				response = response.header(http::header::CONTENT_TYPE, content_type);
			}
			let response = response.body(body).unwrap();
			let app = Router::new().route(
				"/image",
				get(move || {
					let response = response.clone();
					async move { response.map(Body::from) }
				}),
			);
			let task = services
				.server
				.runtime()
				.spawn(async move { axum::serve(listener, app).await.unwrap() });
			Self { url, task }
		}
	}

	impl Drop for ImageServer {
		fn drop(&mut self) { self.task.abort(); }
	}

	#[tokio::test]
	async fn preview_images_store_mime_type_and_dimensions() {
		let (_directory, services) = services().await;
		for format in [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::Gif, ImageFormat::WebP] {
			for content_type in [
				Some(format.to_mime_type()),
				None,
				Some("application/octet-stream"),
				Some("image/svg+xml"),
			] {
				let bytes = image(format);
				let server =
					ImageServer::new(&services, StatusCode::OK, content_type, bytes.clone())
						.await;
				let preview = services
					.media
					.download_image(
						&server.url,
						Some(UrlPreviewData {
							title: Some("Example preview".into()),
							image_width: Some(99),
							image_height: Some(99),
							..UrlPreviewData::default()
						}),
					)
					.await
					.unwrap();
				assert_eq!((preview.image_width, preview.image_height), (Some(2), Some(3)));
				assert_eq!(preview.title.as_deref(), Some("Example preview"));
				let uri = preview.image.unwrap();
				let mxc = Mxc::try_from(uri.as_str()).unwrap();
				let stored = services.media.get(&mxc).await.unwrap().unwrap();
				assert_eq!(stored.content_type.as_deref(), Some(format.to_mime_type()));
				assert_eq!(stored.content.unwrap(), bytes);
			}
		}
	}

	#[tokio::test]
	async fn preview_images_reject_failed_or_invalid_downloads_before_storage() {
		let (_directory, services) = services().await;
		let png = image(ImageFormat::Png);
		let cases = [
			(StatusCode::FOUND, png.clone()),
			(StatusCode::FORBIDDEN, png.clone()),
			(StatusCode::NOT_FOUND, b"<html>Not found</html>".to_vec()),
			(StatusCode::INTERNAL_SERVER_ERROR, png.clone()),
			(StatusCode::OK, b"<html>Access denied</html>".to_vec()),
			(StatusCode::OK, Vec::new()),
			(StatusCode::OK, png[..33].to_vec()),
			(StatusCode::OK, png[..png.len() - 20].to_vec()),
			(StatusCode::OK, vec![0; 4097]),
		];
		for (status, bytes) in cases {
			let server = ImageServer::new(&services, status, Some("image/png"), bytes).await;
			assert!(
				services
					.media
					.download_image(&server.url, None)
					.await
					.is_err(),
				"accepted failed or invalid image with status {status}",
			);
			assert!(services.media.get_all_mxcs().await.unwrap().is_empty());
		}
	}

	#[tokio::test]
	async fn preview_images_reject_excessive_decoded_dimensions_and_memory() {
		let (_directory, services) = services().await;
		for (width, height) in [(8193_u16, 1_u16), (5000, 5000)] {
			let mut bytes = image(ImageFormat::Gif);
			bytes[6..8].copy_from_slice(&width.to_le_bytes());
			bytes[8..10].copy_from_slice(&height.to_le_bytes());
			let server =
				ImageServer::new(&services, StatusCode::OK, Some("image/gif"), bytes).await;
			assert!(
				services
					.media
					.download_image(&server.url, None)
					.await
					.is_err()
			);
			assert!(services.media.get_all_mxcs().await.unwrap().is_empty());
		}
	}
}

#[tokio::test]
#[cfg(disable)] //TODO: fixme
async fn long_file_names_works() {
	use std::path::PathBuf;

	use base64::{Engine as _, engine::general_purpose};

	use super::*;

	struct MockedKVDatabase;

	impl Data for MockedKVDatabase {
		fn create_file_metadata(
			&self,
			_sender_user: Option<&str>,
			mxc: String,
			width: u32,
			height: u32,
			content_disposition: Option<&str>,
			content_type: Option<&str>,
		) -> Result<Vec<u8>> {
			// copied from src/database/key_value/media.rs
			let mut key = mxc.as_bytes().to_vec();
			key.push(0xFF);
			key.extend_from_slice(&width.to_be_bytes());
			key.extend_from_slice(&height.to_be_bytes());
			key.push(0xFF);
			key.extend_from_slice(
				content_disposition
					.as_ref()
					.map(|f| f.as_bytes())
					.unwrap_or_default(),
			);
			key.push(0xFF);
			key.extend_from_slice(
				content_type
					.as_ref()
					.map(|c| c.as_bytes())
					.unwrap_or_default(),
			);

			Ok(key)
		}

		fn delete_file_mxc(&self, _mxc: String) -> Result<()> { todo!() }

		fn search_mxc_metadata_prefix(&self, _mxc: String) -> Result<Vec<Vec<u8>>> { todo!() }

		fn get_all_media_keys(&self) -> Vec<Vec<u8>> { todo!() }

		fn search_file_metadata(
			&self,
			_mxc: String,
			_width: u32,
			_height: u32,
		) -> Result<(Option<String>, Option<String>, Vec<u8>)> {
			todo!()
		}

		fn remove_url_preview(&self, _url: &str) -> Result<()> { todo!() }

		fn set_url_preview(
			&self,
			_url: &str,
			_data: &UrlPreviewData,
			_timestamp: std::time::Duration,
		) -> Result<()> {
			todo!()
		}

		fn get_url_preview(&self, _url: &str) -> Option<UrlPreviewData> { todo!() }
	}

	let db: Arc<MockedKVDatabase> = Arc::new(MockedKVDatabase);
	let mxc = "mxc://example.com/ascERGshawAWawugaAcauga".to_owned();
	let width = 100;
	let height = 100;
	let content_disposition = "attachment; filename=\"this is a very long file name with spaces \
	                           and special characters like äöüß and even emoji like 🦀.png\"";
	let content_type = "image/png";
	let key = db
		.create_file_metadata(
			None,
			mxc,
			width,
			height,
			Some(content_disposition),
			Some(content_type),
		)
		.unwrap();
	let mut r = PathBuf::from("/tmp/media");
	// r.push(base64::encode_config(key, base64::URL_SAFE_NO_PAD));
	// use the sha256 hash of the key as the file name instead of the key itself
	// this is because the base64 encoded key can be longer than 255 characters.
	r.push(general_purpose::URL_SAFE_NO_PAD.encode(<sha2::Sha256 as sha2::Digest>::digest(key)));
	// Check that the file path is not longer than 255 characters
	// (255 is the maximum length of a file path on most file systems)
	assert!(
		r.to_str().unwrap().len() <= 255,
		"File path is too long: {}",
		r.to_str().unwrap().len()
	);
}
