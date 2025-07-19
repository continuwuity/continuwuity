use std::sync::Arc;

use super::{
	super::{Level, fmt},
	Closure, Data,
};
use crate::{Result, SyncMutex};

pub fn fmt_html<S>(out: Arc<SyncMutex<S>>) -> Box<Closure>
where
	S: std::fmt::Write + Send + 'static,
{
	fmt(fmt::html, out)
}

pub fn fmt_markdown<S>(out: Arc<SyncMutex<S>>) -> Box<Closure>
where
	S: std::fmt::Write + Send + 'static,
{
	fmt(fmt::markdown, out)
}

pub fn fmt<F, S>(fun: F, out: Arc<SyncMutex<S>>) -> Box<Closure>
where
	F: Fn(&mut S, &Level, &str, &str) -> Result<()> + Send + Sync + Copy + 'static,
	S: std::fmt::Write + Send + 'static,
{
	Box::new(move |data| call(fun, &mut *out.lock(), &data))
}

fn call<F, S>(fun: F, out: &mut S, data: &Data<'_>)
where
	F: Fn(&mut S, &Level, &str, &str) -> Result<()>,
	S: std::fmt::Write,
{
	fun(out, &data.level(), data.span_name(), data.message()).expect("log line appended");
}
