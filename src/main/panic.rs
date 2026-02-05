use std::{backtrace::Backtrace, panic};

/// Initialize the panic hook to capture backtraces at the point of panic.
/// This is needed to capture the backtrace before the unwind destroys it.
pub(crate) fn init() {
	let default_hook = panic::take_hook();

	panic::set_hook(Box::new(move |info| {
		let backtrace = Backtrace::force_capture();

		let location_str = info.location().map_or_else(String::new, |loc| {
			format!(" at {}:{}:{}", loc.file(), loc.line(), loc.column())
		});

		let message = if let Some(s) = info.payload().downcast_ref::<&str>() {
			(*s).to_owned()
		} else if let Some(s) = info.payload().downcast_ref::<String>() {
			s.clone()
		} else {
			"Box<dyn Any>".to_owned()
		};

		let thread_name = std::thread::current()
			.name()
			.map_or_else(|| "<unnamed>".to_owned(), ToOwned::to_owned);

		eprintln!(
			"\nthread '{thread_name}' panicked{location_str}: \
			 {message}\n\nBacktrace:\n{backtrace}"
		);

		default_hook(info);
	}));
}
