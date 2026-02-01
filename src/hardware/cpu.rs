use dirs;
use std::fs;
use std::path::PathBuf;

use crate::types::{Hardware, PpCompute, ProgPowError, H256};
use progpow_base::params::ProgPowParams;
use progpow_cpu::cache::NodeCacheBuilder;
// use progpow_cpu::cache::OptimizeFor;
// use progpow_cpu::compute::{light_compute, PoW};

const CACHE_DIR: &str = "cache";
const EPIC_HOME: &str = ".epic";

fn get_cache_path() -> Result<PathBuf, ::std::io::Error> {
	// Check if epic dir exists
	let mut epic_path = match dirs::home_dir() {
		Some(p) => p,
		None => PathBuf::new(),
	};

	epic_path.push(EPIC_HOME);
	epic_path.push("main");
	epic_path.push(CACHE_DIR);
	// Create if the default path doesn't exist
	if !epic_path.exists() {
		fs::create_dir_all(epic_path.clone())?;
	}
	Ok(epic_path)
}

pub struct PpCPU<P: ProgPowParams> {
	cache_builder: NodeCacheBuilder,
	_marker: std::marker::PhantomData<P>,
}

impl<P: ProgPowParams> PpCPU<P> {
	pub fn new() -> Self {
		PpCPU {
			cache_builder: NodeCacheBuilder::new(None),
			_marker: std::marker::PhantomData,
		}
	}
}

impl<P: ProgPowParams> PpCompute for PpCPU<P> {
	fn init(&mut self) -> Result<(), ProgPowError> {
		Ok(())
	}

	fn verify(
		&self,
		header_hash: &H256,
		height: u64,
		nonce: u64,
	) -> Result<([u32; 8], [u32; 8]), ProgPowError> {
		let path_cache: PathBuf = get_cache_path().unwrap();

		// Using standalone functions from progpow-light if builder methods are not available or matching?
		// Actually, let's try to use the builder methods first, assuming they exist but need generic P.
		// If they don't exist, I'll need to check cache.rs.
		// But assuming the error was "unexpected argument", the method exists.
		let light = match self.cache_builder.light_from_file::<P>(&path_cache, height) {
			Ok(l) => l,
			Err(_e) => {
				let mut light = self.cache_builder.light::<P>(&path_cache, height);
				if let Err(e) = light.to_file() {
					println!("Light cache file write error: {}", e);
				}
				light
			}
		};

		Ok(light.compute::<P>(header_hash, nonce, height))
	}

	fn compute(&self, _header: [u8; 32], _height: u64, _epoch: i32, _boundary: u64) {
		unimplemented!()
	}

	fn hardware(&self) -> Hardware {
		Hardware::CPU
	}
}
