use std::collections::HashSet;

use rustyline::{DefaultEditor, Result, error::ReadlineError};
use stitcher::{Batch, EventEdges, Stitcher, memory_backend::MemoryStitcherBackend};

const BANNER: &str = "
stitched ordering test repl
- append an event by typing its name: `A`
- to add prev events, type an arrow and then space-separated event names: `A --> B C D`
- to add multiple events at once, separate them with commas
- use `/reset` to clear the ordering
Ctrl-D to exit, Ctrl-C to clear input
"
.trim_ascii();

enum Command<'line> {
	AppendEvents(EventEdges<'line>),
	ResetOrder,
}

peg::parser! {
	// partially copied from the test case parser
	grammar command_parser() for str {
		/// Parse whitespace.
		rule _ -> () = quiet! { $([' '])* {} }

		/// Parse an event ID.
		rule event_id() -> &'input str
			= quiet! { id:$([char if char.is_ascii_alphanumeric() || ['_', '-'].contains(&char)]+) { id } }
			  / expected!("an event ID containing only [a-zA-Z0-9_-]")

		/// Parse an event and its prev events.
		rule event() -> (&'input str, HashSet<&'input str>)
			= id:event_id() prev_events:(_ "-->" _ id:(event_id() ++ _) { id })? {
				(id, prev_events.into_iter().flatten().collect())
			}

		pub rule command() -> Command<'input> =
			"/reset" { Command::ResetOrder }
			/ events:event() ++ (_ "," _) { Command::AppendEvents(events.into_iter().collect()) }
	}
}

fn main() -> Result<()> {
	let mut backend = MemoryStitcherBackend::default();
	let mut reader = DefaultEditor::new()?;

	println!("{BANNER}");

	loop {
		match reader.readline("> ") {
			| Ok(line) => match command_parser::command(&line) {
				| Ok(Command::AppendEvents(events)) => {
					let batch = Batch::from_edges(&events);
					let stitcher = Stitcher::new(&backend);
					let updates = stitcher.stitch(&batch);

					for update in &updates.gap_updates {
						println!("update to gap {}:", update.key);
						println!("    new gap contents: {:?}", update.gap);
						println!("    inserted items: {:?}", update.inserted_items);
					}

					println!("events added to gaps: {:?}", &updates.events_added_to_gaps);
					println!();
					println!("items to sync: {:?}", &updates.new_items);
					backend.extend(updates);
					println!("order: {backend:?}");
				},
				| Ok(Command::ResetOrder) => {
					backend.clear();
					println!("order cleared.");
				},
				| Err(parse_error) => {
					println!("parse error!! {parse_error}");
				},
			},
			| Err(ReadlineError::Interrupted) => {
				println!("interrupt");
			},
			| Err(ReadlineError::Eof) => {
				println!("goodbye :3");
				break Ok(());
			},
			| Err(err) => break Err(err),
		}
	}
}
