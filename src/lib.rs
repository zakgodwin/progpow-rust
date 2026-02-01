pub mod generator;
pub mod hardware;
pub mod types;

use progpow_base::compute::calculate_dag_item;
use progpow_base::params::ProgPowParams;
use progpow_base::shared::Node;

pub const PROGPOW_CACHE_WORDS: usize = 4096;
pub type CDag = [u32; PROGPOW_CACHE_WORDS];

/// Generate L1 cache (c_dag) from Light cache (L2)
pub fn generate_cdag<P: ProgPowParams>(cache: &[Node]) -> CDag {
	let mut c_dag = [0u32; PROGPOW_CACHE_WORDS];
	for i in 0..PROGPOW_CACHE_WORDS / 16 {
		let node = calculate_dag_item::<P>(i as u32, cache);
		for j in 0..16 {
			c_dag[i * 16 + j] = node.as_words()[j];
		}
	}
	c_dag
}

#[cfg(test)]
mod test {
	use super::*;

	use hardware::PpCPU;
	use num_bigint::BigUint;
	// use num_traits::One;
	use types::PpCompute;

	#[test]
	fn test_compute_cpu() {
		let height: u64 = 20;
		let nonce: u64 = 10123012301;
		let header_hash: [u8; 32] = [0; 32];
		let pp_cpu = PpCPU::<progpow_base::params::KawPowParams>::new();
		let (_, mix) = pp_cpu.verify(&header_hash, height, nonce).unwrap();
		assert_eq!(
			mix,
			[
				2257276933, 1807452103, 2437354717, 3964690328, 2418543553, 1799256823, 2347030976,
				2107140455
			]
		);
	}

	#[test]
	#[cfg(any(feature = "cuda", feature = "opencl"))]
	fn test_compute_gpu() {
		use progpow_gpu::utils::get_gpu_solution;

		let header = [20u8; 32];
		let epoch: i32 = 0;
		let height: u64 = 1;
		let boundary: u64 = 100000000;

		let mut difficulty: BigUint = BigUint::from(1u64) << 256; // Equivalent to U256::max_value()
		difficulty = difficulty / BigUint::from(boundary);
		let target: BigUint = difficulty >> 192;

		let (nonce, mix) = get_gpu_solution(header.clone(), height, epoch, boundary);
		let cpu = PpCPU::<progpow_base::params::KawPowParams>::new();
		let (value, mix_hash) = cpu.verify(&header, height, nonce).unwrap();

		let mix32: [u32; 8] = unsafe { ::std::mem::transmute(mix) };
		let target_val: u64 = target.to_u64_digits().first().copied().unwrap_or(0);
		let value_val: u64 = ((value[0] as u64) << 32) | (value[1] as u64);

		assert_eq!(mix32, mix_hash);
		assert!(value_val < target_val);
	}

	#[test]
	fn test_zano_mainnet_accepted_share() {
		use progpow_base::params::ProgPowParams;
		// Data from accepts/herominer.com_zano.accept
		// Work response: height 0x35d5b2 (3528114), header 0x470d42a9...
		// Submit work: nonce 0x00b9d8551f134d3e, mix 0xdfb12430... (Accepted)

		let height: u64 = 3528114;
		let nonce: u64 = 0x00b9d8551f134d3e;
		let header_hex = "470d42a9f6ea35569d6aa7206cf1d4b292a1bc11b0165523f95bbb8678c85d0e";
		let expected_mix_hex = "dfb1243065d51312900ac5fdc67e0b9d6970934871a82ade9f010bfa2894d84f";

		let mut header = [0u8; 32];
		for i in 0..32 {
			header[i] = u8::from_str_radix(&header_hex[i * 2..i * 2 + 2], 16).unwrap();
		}

		let mut expected_mix_bytes = [0u8; 32];
		for i in 0..32 {
			expected_mix_bytes[i] =
				u8::from_str_radix(&expected_mix_hex[i * 2..i * 2 + 2], 16).unwrap();
		}

		let pp_cpu = PpCPU::<progpow_base::params::ZanoParams>::new();
		let (_, mix) = pp_cpu.verify(&header, height, nonce).unwrap();

		let mut actual_mix_bytes = [0u8; 32];
		for (i, &word) in mix.iter().enumerate() {
			actual_mix_bytes[i * 4..(i + 1) * 4].copy_from_slice(&word.to_le_bytes());
		}

		println!("Actual Mix Bytes:   {:02x?}", actual_mix_bytes);
		println!("Expected Mix Bytes: {:02x?}", expected_mix_bytes);

		assert_eq!(
			actual_mix_bytes, expected_mix_bytes,
			"CPU MixHash BYTES should match Pool Accepted MixHash for Zano Mainnet"
		);
	}
}
