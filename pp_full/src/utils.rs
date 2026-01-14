use std::{thread, time};
use types::{Driver, GPU};

pub fn get_gpu_solution(header: [u8; 32], height: u64, epoch: i32, target: u64) -> (u64, [u8; 32]) {
	let mut pp_gpu = GPU::new(0, Driver::OCL);

	pp_gpu.init();
	let ten_millis = time::Duration::from_millis(100);

	loop {
		pp_gpu.compute(header, height, epoch, target, 0);

		thread::sleep(ten_millis);

		let solution = pp_gpu.solutions().unwrap();

		if let Some(sol) = solution {
			return sol;
		}
	}
}

