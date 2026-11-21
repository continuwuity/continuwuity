mod items;
mod items_rev;
mod keys;
mod keys_rev;

use std::sync::Arc;

use conduwuit::{Result, utils::exchange};
use rocksdb::{DBRawIteratorWithThreadMode, ReadOptions};

pub(crate) use self::{items::Items, items_rev::ItemsRev, keys::Keys, keys_rev::KeysRev};
use crate::{
	Map, Slice,
	engine::Db,
	keyval::{Key, KeyVal, Val},
	util::{is_incomplete, map_err},
};

pub(crate) struct State<'a> {
	inner: Inner<'a>,
	seek: bool,
	init: bool,
}

pub(crate) trait Cursor<'a, T> {
	fn state(&self) -> &State<'a>;

	fn fetch(&self) -> Option<T>;

	fn seek(&mut self);

	#[inline]
	fn get(&self) -> Option<Result<T>> {
		self.fetch()
			.map(Ok)
			.or_else(|| self.state().status().map(map_err).map(Err))
	}

	#[inline]
	fn seek_and_get(&mut self) -> Option<Result<T>> {
		self.seek();
		self.get()
	}
}

type Inner<'a> = DBRawIteratorWithThreadMode<'a, Db>;
type From<'a> = Option<Key<'a>>;

impl<'a> State<'a> {
	#[inline]
	pub(super) fn new(map: &'a Arc<Map>, opts: ReadOptions) -> Self {
		Self {
			inner: map.db().db.raw_iterator_cf_opt(&map.cf(), opts),
			init: true,
			seek: false,
		}
	}

	#[inline]
	#[tracing::instrument(level = "trace", skip_all)]
	pub(super) fn init_fwd(mut self, from: From<'_>) -> Self {
		debug_assert!(self.init, "init must be set to make this call");
		debug_assert!(!self.seek, "seek must not be set to make this call");

		if let Some(key) = from {
			self.inner.seek(key);
		} else {
			self.inner.seek_to_first();
		}

		self.seek = true;
		self
	}

	#[inline]
	#[tracing::instrument(level = "trace", skip_all)]
	pub(super) fn init_rev(mut self, from: From<'_>) -> Self {
		debug_assert!(self.init, "init must be set to make this call");
		debug_assert!(!self.seek, "seek must not be set to make this call");

		if let Some(key) = from {
			self.inner.seek_for_prev(key);
		} else {
			self.inner.seek_to_last();
		}

		self.seek = true;
		self
	}

	#[inline]
	#[cfg_attr(unabridged, tracing::instrument(level = "trace", skip_all))]
	pub(super) fn seek_fwd(&mut self) {
		if !exchange(&mut self.init, false) {
			self.inner.next();
		} else if !self.seek {
			self.inner.seek_to_first();
		}
	}

	#[inline]
	#[cfg_attr(unabridged, tracing::instrument(level = "trace", skip_all))]
	pub(super) fn seek_rev(&mut self) {
		if !exchange(&mut self.init, false) {
			self.inner.prev();
		} else if !self.seek {
			self.inner.seek_to_last();
		}
	}

	pub(super) fn is_incomplete(&self) -> bool {
		matches!(self.status(), Some(e) if is_incomplete(&e))
	}

	#[inline]
	fn fetch_key(&self) -> Option<Key<'_>> { self.inner.key() }

	#[inline]
	fn _fetch_val(&self) -> Option<Val<'_>> { self.inner.value() }

	#[inline]
	fn fetch(&self) -> Option<KeyVal<'_>> { self.inner.item() }

	#[inline]
	pub(super) fn status(&self) -> Option<rocksdb::Error> { self.inner.status().err() }

	#[inline]
	pub(super) fn valid(&self) -> bool { self.inner.valid() }
}

fn keyval_longevity<'a, 'b: 'a>(item: KeyVal<'a>) -> KeyVal<'b> {
	(slice_longevity::<'a, 'b>(item.0), slice_longevity::<'a, 'b>(item.1))
}

fn slice_longevity<'a, 'b: 'a>(item: &'a Slice) -> &'b Slice {
	// SAFETY: the lifetime of the data returned by
	// the RocksDB cursor is only valid until the next movement of the cursor,
	// but is unsafely extended here to match the lifetime `'b` of the `Map`
	// borrow because `futures::Stream` is not a lending stream.
	//
	// Callers MUST NOT hold borrowed items (or borrowed deserialized views such
	// as `&str`, `&UserId`, or `&[u8]`) across subsequent polls of the stream —
	// in particular, never call `collect()`, `ready_chunks()`, `peekable()`,
	// `buffered()`, `buffer_unordered()`, `broadband`, or `wideband` on a
	// stream yielding borrowed references without first converting items to
	// owned values (e.g. `map(ToOwned::to_owned)`). The compiler does NOT
	// reject such escapes when items borrow from `'b`.
	//
	// TODO: Fix this, it is old code and language changes may have made it
	// unnecessary
	unsafe { std::mem::transmute(item) }
}
