// Copyright 2021-2022 Parity Technologies (UK) Ltd.
// This file is dual-licensed as Apache-2.0 or MIT.

//! Model check without reference counting:
//! Checks that a sequence of operations and restarts behaves the same as an in-memory collection.

#![no_main]
use libfuzzer_sys::fuzz_target;
use parity_db_fuzz::*;
use std::{collections::HashMap, path::Path};

const NUMBER_OF_POSSIBLE_KEYS: usize = 256;

#[derive(Clone, Debug)]
struct Layer {
	values: [Option<Option<u8>>; NUMBER_OF_POSSIBLE_KEYS], /* For each key the value if it is
	                                                        * inserted or None if it is removed */
	written: bool,
}

type Model = Vec<Layer>;

struct Simulator;

impl DbSimulator for Simulator {
	type Operation = (u8, Option<u8>);
	type Model = Model;

	fn build_options(config: &Config, path: &Path) -> parity_db::Options {
		parity_db::Options {
			path: path.to_owned(),
			columns: vec![parity_db::ColumnOptions {
				compression: config.compression.into(),
				btree_index: config.btree_index,
				..parity_db::ColumnOptions::default()
			}],
			sync_wal: true,
			sync_data: true,
			stats: false,
			salt: None,
			compression_threshold: HashMap::new(),
			always_flush: true,
			with_background_thread: false,
		}
	}

	fn apply_operations_on_model<'a>(
		operations: impl IntoIterator<Item = &'a (u8, Option<u8>)>,
		model: &mut Model,
	) {
		let mut values = [None; NUMBER_OF_POSSIBLE_KEYS];
		for (k, v) in operations {
			values[usize::from(*k)] = Some(*v);
		}
		model.push(Layer { values, written: false });
	}

	fn write_first_layer_to_disk(model: &mut Model) {
		for layer in model {
			if !layer.written {
				layer.written = true;
				break
			}
		}
	}

	fn attempt_to_reset_model_to_disk_state(model: &Model, state: &[(u8, u8)]) -> Option<Model> {
		let mut model = model.clone();
		let expected = {
			let mut values = [None; NUMBER_OF_POSSIBLE_KEYS];
			for (k, v) in state {
				values[usize::from(*k)] = Some(*v);
			}
			values
		};

		while !model.is_empty() {
			if !model.last().unwrap().written {
				model.pop();
				continue
			}

			// Is it equal to current state?
			let mut is_equal = true;
			for (k, expected_value) in expected.iter().enumerate() {
				for layer in model.iter().rev() {
					if let Some(v) = layer.values[k] {
						if v != *expected_value {
							is_equal = false;
						}
						break
					}
				}
			}
			if is_equal {
				// We found it!
				return Some(model)
			}
			log::debug!("Reverting layer number {}", model.len() - 1);
			model.pop();
		}
		if state.is_empty() {
			Some(Vec::new()) // empty state
		} else {
			None
		}
	}

	fn map_operation(operation: &(u8, Option<u8>)) -> parity_db::Operation<Vec<u8>, Vec<u8>> {
		let (k, v) = operation;
		if let Some(v) = *v {
			parity_db::Operation::Set(vec![*k], vec![v])
		} else {
			parity_db::Operation::Dereference(vec![*k])
		}
	}

	fn model_required_content(model: &Model) -> Vec<(Vec<u8>, Vec<u8>)> {
		let mut content = Vec::new();
		for k in u8::MIN..=u8::MAX {
			for layer in model.iter().rev() {
				if let Some(v) = layer.values[usize::from(k)] {
					if let Some(v) = v {
						content.push((vec![k], vec![v]));
					}
					break
				}
			}
		}
		content
	}

	fn model_optional_content(model: &Model) -> Vec<(Vec<u8>, Vec<u8>)> {
		let mut content = Vec::new();
		for k in u8::MIN..=u8::MAX {
			for layer in model.iter().rev() {
				if let Some(v) = layer.values[usize::from(k)] {
					if let Some(v) = v {
						content.push((vec![k], vec![v]));
					}
					break
				}
			}
		}
		content
	}

	fn model_removed_content(model: &Model) -> Vec<Vec<u8>> {
		let mut keys = Vec::new();
		for k in u8::MIN..=u8::MAX {
			for layer in model.iter().rev() {
				if let Some(v) = layer.values[usize::from(k)] {
					if v.is_none() && !layer.written {
						keys.push(vec![k]);
					}
					break
				}
			}
		}
		keys
	}
}

fuzz_target!(|entry: (Config, Vec<Action<(u8, Option<u8>)>>)| {
	let (config, actions) = entry;
	Simulator::simulate(config, actions);
});
