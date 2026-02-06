use std::{thread, time::Duration};

/// Runs a loop that checks for deadlocks every 10 seconds.
///
/// Note that this requires the `deadlock_detection` parking_lot feature to be
/// enabled.
pub(crate) fn deadlock_detection_thread() {
	loop {
		thread::sleep(Duration::from_secs(10));
		let deadlocks = parking_lot::deadlock::check_deadlock();
		if deadlocks.is_empty() {
			continue;
		}

		eprintln!("{} deadlocks detected", deadlocks.len());
		for (i, threads) in deadlocks.iter().enumerate() {
			eprintln!("Deadlock #{i}");
			for t in threads {
				eprintln!("Thread Id {:#?}", t.thread_id());
				eprintln!("{:#?}", t.backtrace());
			}
		}
	}
}

/// Spawns the deadlock detection thread.
///
/// This thread will run in the background and check for deadlocks every 10
/// seconds. When a deadlock is detected, it will print detailed information to
/// stderr.
pub(crate) fn spawn() {
	thread::Builder::new()
		.name("deadlock_detector".to_owned())
		.spawn(deadlock_detection_thread)
		.expect("failed to spawn deadlock detection thread");
}
