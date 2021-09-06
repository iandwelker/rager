use crate::{
	err,
	sync_dir,
	search::entries_with_filter,
	filter::Filter,
	config::Config
};
use std::{
	sync::Arc,
	fs
};

pub async fn remove_with_terms(filter: Filter, config: Config) {
	let filter_arc = Arc::new(filter);
	let config_arc = Arc::new(config);

	let log_dir = sync_dir();

	if let Some(entries) = entries_with_filter(&filter_arc, &config_arc).await {

		// if there are none, tell them
		if entries.is_empty() {
			println!("Your conditions did not turn up any results :(");
		}

		// else go through each and delete their entire directory
		for e in entries.into_iter() {
			let mut entry_dir = log_dir.clone();
			entry_dir.push(e.date_time());

			match std::fs::remove_dir_all(&entry_dir) {
				Err(err) => err!("Could not remove logs at {:?}: {}", entry_dir, err),
				_ => println!("Deleted entry at {:?}", entry_dir)
			}
		}
	}

	// closure for mapping oks to somes
	let ok_to_some = | c: Result<_, _> | {
		match c {
			Ok(co) => Some(co),
			_ => None
		}
	};

	// go back over all the days and remove the directory if there are no more entries in there
	if let Ok(contents) = fs::read_dir(&log_dir) {
		for dir in contents.filter_map(ok_to_some) {
			if let Ok(inner) = fs::read_dir(dir.path()) {
				// only delete the directory if it's empty
				if inner.filter_map(ok_to_some).next().is_none() {
					let path = dir.path();

					if let Err(err) = fs::remove_dir_all(&path) {
						err!("Failed to remove directory at {:?}: {}", path, err);
					}
				}
			}
		}
	}
}
