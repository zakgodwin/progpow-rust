use std::{thread, time};

extern crate libc;

pub mod ffi;
pub mod types;
pub mod utils;

pub use types::{Driver, GPU};

use libc::c_void;

#[cfg(test)]
mod test {
	use super::*;

	#[test]
	#[cfg(feature = "opencl")]
	fn test_compute_ocl() {
		let height: u64 = 1;
		let epoch: i32 = 0;
		let boundary: u64 = 10000;
		let header_hash: [u8; 32] = [20; 32];

		let mut pp_gpu = GPU::new(0, Driver::OCL);

		pp_gpu.init();
		let ten_millis = time::Duration::from_millis(100);

		loop {
			pp_gpu.compute(header_hash, height, epoch, boundary, 0);
			thread::sleep(ten_millis);

			let solution = pp_gpu.solutions().unwrap();

			if let Some(s) = solution {
				let (nonce, mix) = s;
				println!("nonce: {:?}", nonce);
				break;
			}
		}
	}

	#[test]
	#[cfg(feature = "cuda")]
	fn test_compute_cuda() {
		let height: u64 = 1;
		let epoch: i32 = 0;
		let boundary: u64 = 10000;
		let header_hash: [u8; 32] = [20; 32];

		let mut pp_gpu = GPU::new(Driver::CUDA);

		pp_gpu.init();
		let ten_millis = time::Duration::from_millis(100);

		loop {
			pp_gpu.compute(header_hash, height, epoch, boundary, 0);
			thread::sleep(ten_millis);

			let solution = pp_gpu.solutions().unwrap();

			if let Some(s) = solution {
				let (nonce, mix) = s;
				println!("nonce: {:?}", nonce);
				break;
			}
		}
	}
}

