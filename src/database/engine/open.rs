use std::{
	collections::BTreeSet,
	path::Path,
	sync::{Arc, atomic::AtomicU32},
};

use conduwuit::{Result, debug, implement, info, warn};
use rocksdb::{ColumnFamilyDescriptor, Options};

use super::{
	Db, Engine,
	cf_opts::cf_options,
	db_opts::db_options,
	descriptor::{self, Descriptor},
	repair::repair,
};
use crate::{Context, or_else};

#[implement(Engine)]
#[tracing::instrument(skip_all, level = "info")]
pub(crate) async fn open(ctx: Arc<Context>, desc: &[Descriptor]) -> Result<Arc<Self>> {
	let server = &ctx.server;
	let config = &server.config;
	let path = &config.database_path;

	let db_opts = db_options(config, &ctx.env.lock(), &ctx.row_cache.lock())?;

	let cfds = Self::configure_cfds(&ctx, &db_opts, desc)?;
	let num_cfds = cfds.len();
	debug!("Configured {num_cfds} column descriptors...");

	let load_time = std::time::Instant::now();
	if config.rocksdb_repair {
		repair(&db_opts, &config.database_path)?;
	}

	debug!("Opening database...");
	let db = Db::open_cf_descriptors(&db_opts, path, cfds).or_else(or_else)?;

	info!(
		columns = num_cfds,
		sequence = %db.latest_sequence_number(),
		time = ?load_time.elapsed(),
		"Opened database."
	);

	Ok(Arc::new(Self {
		db,
		pool: ctx.pool.clone(),
		ctx: ctx.clone(),
		checksums: config.rocksdb_checksums,
		corks: AtomicU32::new(0),
	}))
}

#[implement(Engine)]
#[tracing::instrument(name = "configure", skip_all, level = "debug")]
fn configure_cfds(
	ctx: &Arc<Context>,
	db_opts: &Options,
	desc: &[Descriptor],
) -> Result<Vec<ColumnFamilyDescriptor>> {
	let server = &ctx.server;
	let config = &server.config;
	let path = &config.database_path;
	let existing = Self::discover_cfs(path, db_opts);

	let creating = desc.iter().filter(|desc| !existing.contains(desc.name));

	let missing = existing
		.iter()
		.filter(|&name| name != "default")
		.filter(|&name| !desc.iter().any(|desc| desc.name == name));

	debug!(
		existing = existing.len(),
		described = desc.len(),
		missing = missing.clone().count(),
		creating = creating.clone().count(),
		"Discovered database columns"
	);

	missing.clone().for_each(|name| {
		debug!("Found unrecognized column {name:?} in existing database.");
	});

	creating.map(|desc| desc.name).for_each(|name| {
		debug!("Creating new column {name:?} not previously found in existing database.");
	});

	let missing_descriptors = missing.clone().map(|_| descriptor::DROPPED);

	let cfopts: Vec<_> = desc
		.iter()
		.copied()
		.chain(missing_descriptors)
		.map(|ref desc| cf_options(ctx, db_opts.clone(), desc))
		.collect::<Result<_>>()?;

	let cfds: Vec<_> = desc
		.iter()
		.map(|desc| desc.name)
		.map(ToOwned::to_owned)
		.chain(missing.cloned())
		.zip(cfopts.into_iter())
		.map(|(name, opts)| ColumnFamilyDescriptor::new(name, opts))
		.collect();

	Ok(cfds)
}

#[implement(Engine)]
#[tracing::instrument(name = "discover", skip_all, level = "debug")]
fn discover_cfs(path: &Path, opts: &Options) -> BTreeSet<String> {
	Db::list_cf(opts, path)
		.unwrap_or_default()
		.into_iter()
		.collect::<BTreeSet<_>>()
}
