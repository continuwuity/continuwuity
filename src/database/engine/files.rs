use conduwuit::Result;
use rocksdb::LiveFile as SstFile;

use crate::util::map_err;

impl super::Engine {
	pub fn file_list(&self) -> impl Iterator<Item = Result<SstFile>> + Send + use<> {
		self.db
			.live_files()
			.map_err(map_err)
			.into_iter()
			.flat_map(Vec::into_iter)
			.map(Ok)
	}
}
