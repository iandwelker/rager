#![allow(non_camel_case_types)]

use crate::*;
use std::{
	path,
	fs,
	sync::{Arc, RwLock}
};
use chrono::Datelike;

pub async fn search(any: bool, user: Option<String>, when: Option<String>, term: Option<String>, view: bool) {
	let dir = sync_dir();

	let matches: Arc<RwLock<Vec<EntryDetails>>> = Arc::new(RwLock::new(Vec::new()));
	let date_regex = regex::Regex::new(r"^\d{4}-\d{2}-\d{2}$").unwrap();

	let day_matches = if let Some(ref days) = when {
		Some(days.split(',')
			.fold(Vec::new(), |mut mtc, day| {
				if date_regex.is_match(day) {
					mtc.push(day.to_owned())
				} else {
					let now = chrono::offset::Utc::now();
					let parse = day.parse::<chrono::Weekday>();

					let days_ago = if day.starts_with("today") {
						Some(0)
					} else if day.starts_with("yesterday") {
						Some(1)
					} else if let Ok(entry) = parse {
						let from_now = now.weekday().num_days_from_sunday();
						let from_then = entry.num_days_from_sunday();

						if from_now != from_then {
							Some((from_now + 7 - from_then) % 7)
						} else {
							Some(7)
						}
					} else {
						None
					};

					if let Some(da) = days_ago {
						if let Some(date_string) = now.with_day(now.day() - da) {
							mtc.push(date_string.format("%Y-%m-%d").to_string());
						}
					}
				}

				mtc
			}))
	} else {
		None
	};

	if when.is_some() && day_matches.is_none() {
		err!("Your 'when' could not be parsed into a date. Please only pass in a day of the week or an ISO-8601 date.");
		return
	}

	let mut dbg_str = "Searching for logs".to_owned();

	if let Some(ref u) = user {
		dbg_str = format!("{} by user \x1b[1m{}\x1b[0m", dbg_str, u);
	}
	if let Some(ref w) = when {
		dbg_str = format!("{} from \x1b[1m{}\x1b[0m", dbg_str, w);
	}
	if let Some(ref t) = term {
		dbg_str = format!("{} containing the term '\x1b[1m{}\x1b[0m'", dbg_str, t);
	}

	println!("{}...\n", dbg_str);

	let days: Vec<String> = if let Ok(contents) = fs::read_dir(&dir) {
		contents.fold(Vec::new(), |mut entries, day_res| {
			// uhhghghghgh. I despise this nestedness
			if let Ok(day) = day_res {
				if let Some(file_name) = day.path().file_name() {
					if let Some(file_str) = file_name.to_str() {
						if let Some(ref d_match) = day_matches {
							for m in d_match {
								if file_name.eq_ignore_ascii_case(m) {
									entries.push(file_str.into());
								}
							}
						} else {
							entries.push(file_str.into())
						}
					}
				}
			}

			entries
		})
	} else {
		return;
	};

	let joins = days.into_iter()
		.fold(Vec::new(), | mut joins, iter_day | {
			let mut dir_clone = dir.clone();
			dir_clone.push(iter_day);
			let match_clone = matches.clone();

			let user_cond = user.as_ref().map(|m| m.to_owned());
			let term_cond = term.as_ref().map(|t| t.to_owned());

			let join = tokio::spawn(async move {

				if let Some(mut entr) = get_entries_for_day(&dir_clone) {
					if !any {
						if let Some(user) = user_cond {
							entr = entr.into_iter()
								.filter(|e| e.user_id.contains(&user))
								.collect();
						}

						if let Some(term) = term_cond {
							let regex = match regex::Regex::new(&term) {
								Ok(rg) => rg,
								Err(err) => {
									err!("Cannot format '{}' into regex: {}", term, err);
									return;
								}
							};

							entr = entr.into_iter()
								.filter(|e| entry_matches_regex(&e, &regex))
								.collect();
						}
					}

					if let Ok(mut c_matches) = match_clone.write() {
						c_matches.append(&mut entr);
					}
				}
			});

			joins.push(join);
			joins
		});

	futures::future::join_all(joins).await;

	if let Ok(finds) = matches.read() {
		if finds.is_empty() {
			println!(":( It looks like your search terms didn't turn up any results");
		} else {
			let descriptions = finds.iter().map(|e| e.selectable_description());

			let mut menu = youchoose::Menu::new(descriptions);
			let choice = menu.show();

			if !choice.is_empty() {
				let entry = &finds[choice[0]];

				if view {
					view::view(entry);
				} else {
					println!("{}", entry.description());
				}
			}
		}
	};
}

fn get_entries_for_day(day: &std::path::PathBuf) -> Option<Vec<EntryDetails>> {
	if let Ok(contents) = fs::read_dir(&day) {
		return Some(contents.fold(Vec::new(), |mut es, t| {

			if let Ok(time) = t {
				if let Some(entry) = get_details_of_entry(&time.path()) {
					es.push(entry);
				}
			}

			es
		}));
	}

	None
}

fn entry_matches_regex(entry: &EntryDetails, rgx: &regex::Regex) -> bool {
	if let Ok(contents) = fs::read_dir(&entry.path) {
		for file_res in contents {
			if let Ok(file) = file_res {
				if !file.path().is_file() {
					continue;
				}

				let text = match fs::read_to_string(&file.path()) {
					Ok(txt) => txt,
					Err(_) => continue,
				};

				if rgx.is_match(&text) {
					return true;
				}
			}
		}
	}

	false
}

pub fn get_details_of_entry(entry: &std::path::PathBuf) -> Option<EntryDetails> {
	if let Some(contents) = get_detail_file_of_entry(entry) {
		let mut details = "";
		let mut user_id = "unknown";
		let mut os = EntryOS::iOS;
		let mut version = "1.1.30".to_owned();
		let mut build: Option<String> = None;

		for (idx, line) in contents.lines().enumerate() {
			if idx == 0 {
				details = line;
			} else if line.starts_with("Application") && line.contains("android") {
				os = EntryOS::Android;
			} else if line.starts_with("user_id") {
				let components: Vec<&str> = line.split(' ').collect();

				if components.len() > 1 {
					user_id = components[1];
				}
			} else if line.starts_with("Version") {
				let components: Vec<&str> = line.split(' ').collect();

				if components.len() > 1 {
					version = components[1].to_owned();
				}
			} else if line.starts_with("build") {
				let components: Vec<&str> = line.split(' ').collect();

				if components.len() > 1 {
					build = Some(components[1..].join(" "));
				}
			}
		}

		if let Some(bd) = build {
			version = format!("{} ({})", version, bd);
		}

		return Some(EntryDetails::new(user_id.into(), os, details.into(), version, entry.to_owned()))
	}

	None
}

fn get_detail_file_of_entry(entry: &std::path::PathBuf) -> Option<String> {
	let mut dir = entry.to_owned();
	dir.push("details.log.gz");

	if !std::path::Path::new(&dir).exists() {
		return None;
	}

	match fs::read_to_string(&dir) {
		Ok(contents) => Some(contents),
		Err(_) => None,
	}
}

pub struct EntryDetails {
	user_id: String,
	os: EntryOS,
	details: String,
	version: String,
	pub path: path::PathBuf
}

impl EntryDetails {
	pub fn new(user_id: String, os: EntryOS, details: String, version: String, path: path::PathBuf) -> EntryDetails {
		EntryDetails {
			user_id,
			os,
			details,
			version,
			path
		}
	}

	pub fn description(&self) -> String {
		format!("\x1b[1m{}\x1b[0m: {}\n\
			\tOS:       \x1b[32;1m{}\x1b[0m\n\
			\tVersion:  \x1b[32;1m{}\x1b[0m\n\
			\tLocation: {:?}\n",
				self.user_id,
				self.details,
				match self.os {
					EntryOS::iOS => "iOS",
					EntryOS::Android => "Android",
				},
				self.version,
				self.path
			)
	}

	pub fn selectable_description(&self) -> String {
		format!("{} ({}): {}",
			self.user_id,
			match self.os {
				EntryOS::iOS => "iOS",
				EntryOS::Android => "Android",
			},
			self.details)
	}
}

pub enum EntryOS {
	iOS,
	Android
}
