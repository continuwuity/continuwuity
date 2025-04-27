use std::{
	collections::{HashMap, hash_map},
	future::Future,
	pin::Pin,
	sync::RwLock,
};

use tokio::sync::watch;

type Watcher = RwLock<HashMap<Vec<u8>, (watch::Sender<()>, watch::Receiver<()>)>>;

#[derive(Default)]
pub(crate) struct Watchers {
	watchers: Watcher,
}

impl Watchers {
	pub(crate) fn watch<'a>(
		&'a self,
		prefix: &[u8],
	) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
		let mut rx = match self.watchers.write().unwrap().entry(prefix.to_vec()) {
			| hash_map::Entry::Occupied(o) => o.get().1.clone(),
			| hash_map::Entry::Vacant(v) => {
				let (tx, rx) = watch::channel(());
				v.insert((tx, rx.clone()));
				rx
			},
		};

		Box::pin(async move {
			// Tx is never destroyed
			rx.changed().await.unwrap();
		})
	}

	pub(crate) fn wake(&self, key: &[u8]) {
		let watchers = self.watchers.read().unwrap();
		let mut triggered = Vec::new();
		for length in 0..=key.len() {
			if watchers.contains_key(&key[..length]) {
				triggered.push(&key[..length]);
			}
		}

		drop(watchers);

		if !triggered.is_empty() {
			let mut watchers = self.watchers.write().unwrap();
			for prefix in triggered {
				if let Some(tx) = watchers.remove(prefix) {
					tx.0.send(()).expect("channel should still be open");
				}
			}
		}
	}
}
