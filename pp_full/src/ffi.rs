extern "C" {
	pub fn progpow_gpu_init(device: u32, driver: u32) -> *mut ::std::os::raw::c_void;
}

extern "C" {
	pub fn progpow_gpu_configure(devicesCount: u32);
}

extern "C" {
	pub fn progpow_gpu_compute(
		miner: *mut ::std::os::raw::c_void,
		header: *const ::std::os::raw::c_void,
		height: u64,
		epoch: i32,
		target: u64,
		start_nonce: u64,
	);
}

extern "C" {
	pub fn progpow_destroy(miner: *mut ::std::os::raw::c_void) -> bool;
}

extern "C" {
	pub fn progpow_gpu_get_solutions(
		miner: *mut ::std::os::raw::c_void,
		data: *mut ::std::os::raw::c_void,
	) -> bool;
}

