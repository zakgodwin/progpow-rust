// Implementation of the CUDA kernel generator for ProgPow/KawPow
// Ported from the official xmrig-cuda implementation (CudaKawPow_gen.cpp & KawPow.h)

// Assuming progpow_base is a sibling crate in the workspace
use progpow_base::params::ProgPowParams;
use std::fmt::Write;

const PROGPOW_REGS: usize = 32;
const PROGPOW_LANES: usize = 16;
const PROGPOW_DAG_LOADS: usize = 4;
const PROGPOW_CACHE_BYTES: usize = 16384;
const PROGPOW_CNT_DAG: usize = 64;

// KISS99 generator
struct Kiss99 {
	z: u32,
	w: u32,
	jsr: u32,
	jcong: u32,
}

impl Kiss99 {
	fn new(z: u32, w: u32, jsr: u32, jcong: u32) -> Self {
		Self { z, w, jsr, jcong }
	}

	fn rnd(&mut self, _is_zano: bool) -> u32 {
		self.z = 36969u32
			.wrapping_mul(self.z & 65535)
			.wrapping_add(self.z >> 16);
		self.w = 18000u32
			.wrapping_mul(self.w & 65535)
			.wrapping_add(self.w >> 16);
		let mwc = (self.z << 16).wrapping_add(self.w);

		self.jcong = self.jcong.wrapping_mul(69069).wrapping_add(1234567);
		// All ProgPow variants use the same SHR3 shifts: 17, 13, 5
		self.jsr ^= self.jsr << 17;
		self.jsr ^= self.jsr >> 13;
		self.jsr ^= self.jsr << 5;

		(mwc ^ self.jcong).wrapping_add(self.jsr)
	}
}

// Helper for FNV1a
fn fnv1a(h: &mut u32, d: u32) -> u32 {
	*h = (*h ^ d).wrapping_mul(0x1000193);
	*h
}

// lazy_static! {
// 	pub static ref KAWPOW_PARAMS: ProgPowParams = ProgPowParams::kawpow();
// }

pub fn generate_cuda_kernel<P: ProgPowParams>(period: u64, _height: u64) -> String {
	let mut code = String::from(PROGPOW_KERNEL_TEMPLATE);

	let prog_seed = P::prog_seed(_height);
	let epoch = _height / P::EPOCH_LENGTH;
	let dag_size = progpow_base::shared::get_data_size::<P>(epoch * P::EPOCH_LENGTH);
	let dag_elements = dag_size / 256;

	// Generate Random Math and DAG Loads logic
	let (random_math, dag_loads) = get_code::<P>(prog_seed);

	// Replace placeholders
	code = code.replace("XMRIG_INCLUDE_PROGPOW_RANDOM_MATH", &random_math);
	code = code.replace("XMRIG_INCLUDE_PROGPOW_DATA_LOADS", &dag_loads);

	// Calculate Fast Modulo Data
	println!("DEBUG: generate_cuda_kernel dag_elements={}", dag_elements);
	let mut mod_logic = String::new();
	if (dag_elements & (dag_elements - 1)) == 0 {
		// Power of two optimization
		let _ = writeln!(mod_logic, "offset &= {};", dag_elements - 1);
	} else {
		let (r, i, s) = calculate_fast_mod_data(dag_elements as u32);
		if i != 0 {
			let _ = writeln!(mod_logic, "const uint32_t offset1 = offset + {};", i);
			let _ = writeln!(mod_logic, "const uint32_t rcp = {};", r);
			let _ = writeln!(
				mod_logic,
				"offset -= ((offset1 ? __umulhi(offset1, rcp) : rcp) >> {}) * {};",
				s - 32,
				dag_elements
			);
		} else {
			let _ = writeln!(
				mod_logic,
				"offset -= (__umulhi(offset, {}) >> {}) * {};",
				r,
				s - 32,
				dag_elements
			);
		}
	}

	println!("GENERATED MOD LOGIC:\n{}", mod_logic);
	code = code.replace("XMRIG_INCLUDE_OFFSET_MOD_DAG_ELEMENTS", &mod_logic);

	// Launch bounds (Hardcoded to 256 threads as per typical usage, or parameterized if needed)
	// xmrig-cuda defaults: blocks=8192, threads=256 usually, but varies by arch.
	// The template has XMRIG_INCLUDE_LAUNCH_BOUNDS.
	// We will set a safe default or use params if available.
	// Assuming 256 threads for now as a safe default for KawPow.
	code = code.replace("XMRIG_INCLUDE_LAUNCH_BOUNDS", "");
	code = code.replace("XMRIG_INCLUDE_KECCAK_ROUNDS", &P::KECCAK_ROUNDS.to_string());

	// Inject Constants derived from params
	// The template uses defines. We should ensuring they match params.
	// Ideally we prepend defines if the template relies on them being unset,
	// but the template below has them defined. We should replace them or ensure they match.
	// For now, we assume standard KawPow params 16/32/4/64/11/18 if params match.
	// If dynamic params are needed, we would replace the #defines in the template string.

	// Replace hardcoded defines if they differ from params
	// Replace hardcoded defines if they differ from params
	// Inject KAWPOW_IS_RAVENCOIN
	// Inject Constants derived from params
	// We use a placeholder XMRIG_INCLUDE_DEFINES to inject all dynamic defines
	let is_zano = P::MATH_MAPPING == progpow_base::params::MathMapping::Zano;
	let defines = format!(
		"#define KAWPOW_IS_RAVENCOIN     {}\n#define PROGPOW_IS_ZANO         {}\n#define PROGPOW_CNT_CACHE       {}\n#define PROGPOW_CNT_MATH        {}\n#define PROGPOW_START_OFFSET    0",
		if P::HAS_RAVENCOIN_RNDC { 1 } else { 0 },
		if is_zano { 1 } else { 0 },
		P::CNT_CACHE,
		P::CNT_MATH
	);
	code = code.replace("XMRIG_INCLUDE_DEFINES", &defines);
	println!("GENERATED DEFINES:\n{}", defines);

	code = code.replace("XMRIG_INCLUDE_KECCAK_ROUNDS", &P::KECCAK_ROUNDS.to_string());
	// Padding Logic Replacement
	let padding_logic = if P::HAS_RAVENCOIN_RNDC {
		"#if KAWPOW_IS_RAVENCOIN\n        for (int i = 10; i < 25; i++)\n            state[i] = ravencoin_rndc[i-10];\n#endif"
	} else if is_zano {
		// Zano uses zero padding
		"        for (int i = 10; i < 25; i++) state[i] = 0;"
	} else {
		// Standard ProgPow uses Keccak padding (0x01 ... 0x80)
		"        for (int i = 10; i < 25; i++) state[i] = 0;\n        state[10] = 0x00000001;\n        state[18] = 0x80008081;"
	};

	println!("GENERATED PADDING LOGIC:\n{}", padding_logic);
	code = code.replace("XMRIG_INCLUDE_PROGPOW_INITIAL_PADDING", padding_logic);

	let is_zano = P::MATH_MAPPING == progpow_base::params::MathMapping::Zano;
	println!(
		"DEBUG: generate_cuda_kernel is_zano={} NAME={}",
		is_zano,
		P::NAME
	);
	let kiss99_logic = r#"
    st.jcong = 69069 * st.jcong + 1234567;
    st.jsr ^= (st.jsr << 17);
    st.jsr ^= (st.jsr >> 13);
    st.jsr ^= (st.jsr << 5);
"#;
	code = code.replace("XMRIG_INCLUDE_KISS99_LOGIC", kiss99_logic);

	// KawPow/Standard ProgPow: direct extraction, no swap (per cpp-kawpow reference)
	// Zano/Sero: swap bytes and reverse order (BE conversion)
	let hash_seed_extract = if P::SEED_BYTE_SWAP {
		// Zano uses be::uint64(h.word64s[0]) which is bswap64 on LE systems
		// This swaps bytes AND reverses word order (index 1 then 0, not 0 then 1)
		// Must match CPU: h_seed = [st_initial[1].swap_bytes(), st_initial[0].swap_bytes()]
		r#"    hash_seed_small[0] = cuda_swab32(state2[1]);
    hash_seed_small[1] = cuda_swab32(state2[0]);"#
	} else {
		// KawPow/Standard uses hash_seed = [state2[0], state2[1]] directly
		r#"    hash_seed_small[0] = state2[0];
    hash_seed_small[1] = state2[1];"#
	};
	println!("DEBUG: hash_seed_extract = {}", hash_seed_extract);
	code = code.replace("XMRIG_INCLUDE_HASH_SEED_EXTRACT", hash_seed_extract);

	code
}

// Logic from xmrig-cuda/CudaKawPow_gen.cpp
fn get_code<P: ProgPowParams>(prog_seed: u64) -> (String, String) {
	let mut random_math = String::with_capacity(4096);
	let mut dag_loads = String::with_capacity(1024);

	let seed0 = prog_seed as u32;
	let seed1 = (prog_seed >> 32) as u32;

	let is_zano = P::MATH_MAPPING == progpow_base::params::MathMapping::Zano;
	// Both KawPow and Zano use FNV-1a chaining for program RNG initialization
	// Reference: progpow-light/src/progpow.rs:progpow_init()
	let mut h = 0x811c9dc5u32; // FNV_HASH
	let z = fnv1a(&mut h, seed0);
	let w = fnv1a(&mut h, seed1);
	let jsr = fnv1a(&mut h, seed0);
	let jcong = fnv1a(&mut h, seed1);
	let mut rng = Kiss99::new(z, w, jsr, jcong);

	let mut mix_seq_dst = (0..PROGPOW_REGS).map(|i| i as i32).collect::<Vec<i32>>();
	let mut mix_seq_cache = (0..PROGPOW_REGS).map(|i| i as i32).collect::<Vec<i32>>();
	let mut mix_seq_dst_cnt = 0;
	let mut mix_seq_cache_cnt = 0;

	for i in (1..PROGPOW_REGS).rev() {
		let j = (rng.rnd(is_zano) as usize) % (i + 1);
		mix_seq_dst.swap(i, j);
		let j = (rng.rnd(is_zano) as usize) % (i + 1);
		mix_seq_cache.swap(i, j);
	}

	// Debug: Print shuffle sequences to verify they match CPU
	println!("DEBUG GPU Generator: prog_seed={}", prog_seed);
	println!(
		"DEBUG GPU Generator: mix_seq_dst[0..4] = {} {} {} {}",
		mix_seq_dst[0], mix_seq_dst[1], mix_seq_dst[2], mix_seq_dst[3]
	);
	println!(
		"DEBUG GPU Generator: mix_seq_cache[0..4] = {} {} {} {}",
		mix_seq_cache[0], mix_seq_cache[1], mix_seq_cache[2], mix_seq_cache[3]
	);

	let cnt_cache = P::CNT_CACHE;
	let cnt_math = P::CNT_MATH;
	let max_ops = std::cmp::max(cnt_cache, cnt_math);

	for i in 0..max_ops {
		if i < cnt_cache {
			let src = format!("mix[{}]", mix_seq_cache[mix_seq_cache_cnt % PROGPOW_REGS]);
			mix_seq_cache_cnt += 1;
			let dest = format!("mix[{}]", mix_seq_dst[mix_seq_dst_cnt % PROGPOW_REGS]);
			mix_seq_dst_cnt += 1;
			let r = rng.rnd(is_zano);

			let _ = writeln!(random_math, "    // cache load {}", i);
			let _ = writeln!(random_math, "    offset = {} % PROGPOW_CACHE_WORDS;", src);
			let _ = writeln!(random_math, "    data = c_dag[offset];");
			random_math.push_str(&merge(&dest, "data", r));
		}

		if i < cnt_math {
			let src_rnd = (rng.rnd(is_zano) as usize) % ((PROGPOW_REGS - 1) * PROGPOW_REGS);
			let src1 = src_rnd % PROGPOW_REGS;
			let mut src2 = src_rnd / PROGPOW_REGS;
			if src2 >= src1 {
				src2 += 1;
			}

			let src1_str = format!("mix[{}]", src1);
			let src2_str = format!("mix[{}]", src2);
			let r1 = rng.rnd(is_zano);

			let dest = format!("mix[{}]", mix_seq_dst[mix_seq_dst_cnt % PROGPOW_REGS]);
			mix_seq_dst_cnt += 1;
			let r2 = rng.rnd(is_zano);

			let _ = writeln!(random_math, "    // random math {}", i);
			random_math.push_str(&math("data", &src1_str, &src2_str, r1, P::MATH_MAPPING));
			random_math.push_str(&merge(&dest, "data", r2));
		}
	}

	// DAG Loads
	dag_loads.push_str(&merge("mix[0]", "data_dag.s[0]", rng.rnd(is_zano)));
	for i in 1..PROGPOW_DAG_LOADS {
		let dest = format!("mix[{}]", mix_seq_dst[mix_seq_dst_cnt % PROGPOW_REGS]);
		mix_seq_dst_cnt += 1;
		let r = rng.rnd(is_zano);
		dag_loads.push_str(&merge(&dest, &format!("data_dag.s[{}]", i), r));
	}

	println!("GENERATED RANDOM MATH:\n{}", random_math);
	println!("GENERATED DAG LOADS:\n{}", dag_loads);
	(random_math, dag_loads)
}

fn calculate_fast_mod_data(divisor: u32) -> (u32, u32, u32) {
	// Ported from calculate_fast_mod_data in CudaKawPow_gen.cpp
	if (divisor & (divisor - 1)) == 0 {
		return (1, 0, 31 - divisor.leading_zeros());
	}

	let shift = 63 - divisor.leading_zeros();
	let n = 1u64 << shift;
	let q = n / (divisor as u64);
	let r_rem = n - q * (divisor as u64);

	let reciprocal;
	let increment;

	if r_rem * 2 < (divisor as u64) {
		reciprocal = q as u32;
		increment = 1;
	} else {
		reciprocal = (q + 1) as u32;
		increment = 0;
	}

	(reciprocal, increment, shift)
}

fn merge(a: &str, b: &str, r: u32) -> String {
	match r % 4 {
		0 => format!("    {} = ({} * 33) + {};\n", a, a, b),
		1 => format!("    {} = ({} ^ {}) * 33;\n", a, a, b),
		2 => format!(
			"    {} = ROTL32({}, {}) ^ {};\n",
			a,
			a,
			((r >> 16) % 31) + 1,
			b
		),
		3 => format!(
			"    {} = ROTR32({}, {}) ^ {};\n",
			a,
			a,
			((r >> 16) % 31) + 1,
			b
		),
		_ => String::from("#error\n"),
	}
}

fn math(d: &str, a: &str, b: &str, r: u32, mapping: progpow_base::params::MathMapping) -> String {
	use progpow_base::params::MathMapping;
	match mapping {
		MathMapping::Standard | MathMapping::KawPow => match r % 11 {
			0 => format!("    {} = {} + {};\n", d, a, b),
			1 => format!("    {} = {} * {};\n", d, a, b),
			2 => format!("    {} = mul_hi({}, {});\n", d, a, b),
			3 => format!("    {} = min({}, {});\n", d, a, b),
			4 => format!("    {} = ROTL32({}, {} % 32);\n", d, a, b),
			5 => format!("    {} = ROTR32({}, {} % 32);\n", d, a, b),
			6 => format!("    {} = {} & {};\n", d, a, b),
			7 => format!("    {} = {} | {};\n", d, a, b),
			8 => format!("    {} = {} ^ {};\n", d, a, b),
			9 => format!("    {} = clz({}) + clz({});\n", d, a, b),
			_ => format!("    {} = popcount({}) + popcount({});\n", d, a, b),
		},
		MathMapping::Zano => match r % 11 {
			0 => format!("    {} = clz({}) + clz({});\n", d, a, b),
			1 => format!("    {} = popcount({}) + popcount({});\n", d, a, b),
			2 => format!("    {} = {} + {};\n", d, a, b),
			3 => format!("    {} = {} * {};\n", d, a, b),
			4 => format!("    {} = mul_hi({}, {});\n", d, a, b),
			5 => format!("    {} = min({}, {});\n", d, a, b),
			6 => format!("    {} = ROTL32({}, {} & 31);\n", d, a, b),
			7 => format!("    {} = ROTR32({}, {} & 31);\n", d, a, b),
			8 => format!("    {} = {} & {};\n", d, a, b),
			9 => format!("    {} = {} | {};\n", d, a, b),
			_ => format!("    {} = {} ^ {};\n", d, a, b),
		},
	}
}

// TODO: The existing opencl generator can validly remain as is or be updated similarly.
// For now, keeping the existing one but patched is safer than deleting it if other code uses it.
// However, the user request focused on rewriting the cuda code.
// I will keep the OpenCL function signature but implement it similarly if needed, or leave the previous fix.
// Given strict instructions, I will apply similar logic to OpenCL if possible, but prioritize CUDA.
// For now, I'll copy the previous OpenCL function back in to avoid breaking the build, as I am replacing the whole file.

pub fn generate_opencl_kernel<P: ProgPowParams>(period: u64, _height: u64) -> String {
	// Re-using the logic for OpenCL? Ideally yes.
	// For now, let's just use the previous implementation to pass compilation,
	// unless the user wants OpenCL fixed too. They said "Rewrite entire cuda related code".
	// I will return a placeholder or the old code.
	// Actually, I can use the template approach for OpenCL too if I had an OpenCL template.
	// I will restore the OLD OpenCL code (with my previous fixes) to ensure no regression there.

	let prog_seed = period;
	let epoch = _height / P::EPOCH_LENGTH;
	let dag_size = progpow_base::shared::get_data_size::<P>(epoch * P::EPOCH_LENGTH);
	let dag_elements = dag_size / 256;

	let seed0 = prog_seed as u32;
	let seed1 = (prog_seed >> 32) as u32;
	let fnv_hash = 0x811c9dc5;
	let mut h = fnv_hash;
	let z = fnv1a(&mut h, seed0);
	let w = fnv1a(&mut h, seed1);
	let jsr = fnv1a(&mut h, seed0);
	let jcong = fnv1a(&mut h, seed1);
	let mut rng = Kiss99::new(z, w, jsr, jcong);

	let mut mix_seq_dst = (0..PROGPOW_REGS).map(|i| i as i32).collect::<Vec<i32>>();
	let mut mix_seq_cache = (0..PROGPOW_REGS).map(|i| i as i32).collect::<Vec<i32>>();
	let mut mix_seq_dst_cnt = 0;
	let mut mix_seq_cache_cnt = 0;

	let is_zano = P::MATH_MAPPING == progpow_base::params::MathMapping::Zano;
	for i in (1..PROGPOW_REGS).rev() {
		let j = (rng.rnd(is_zano) as usize) % (i + 1);
		mix_seq_dst.swap(i, j);
		let j = (rng.rnd(is_zano) as usize) % (i + 1);
		mix_seq_cache.swap(i, j);
	}

	let mut inner_code = String::new();
	inner_code.push_str("#pragma OPENCL EXTENSION cl_khr_subgroups : enable\n");
	inner_code.push_str("#pragma OPENCL EXTENSION cl_khr_int64_base_atomics : enable\n\n");
	inner_code.push_str("#define ROTL32(x,n) rotate((uint)(x), (uint)(n))\n");
	inner_code.push_str("#define ROTR32(x,n) rotate((uint)(x), (uint)(32-(n)))\n");
	inner_code.push_str("#define mul_hi(a, b) mul_hi((uint)(a), (uint)(b))\n");
	inner_code.push_str("#define clz(a) clz((uint)(a))\n");
	inner_code.push_str("#define popcount(a) popcount((uint)(a))\n\n");

	let _ = writeln!(
		inner_code,
		"#define PROGPOW_LANES           {}",
		PROGPOW_LANES
	);
	let _ = writeln!(
		inner_code,
		"#define PROGPOW_REGS            {}",
		PROGPOW_REGS
	);
	let _ = writeln!(
		inner_code,
		"#define PROGPOW_DAG_LOADS       {}",
		PROGPOW_DAG_LOADS
	);
	let _ = writeln!(
		inner_code,
		"#define PROGPOW_CACHE_WORDS     {}",
		PROGPOW_CACHE_BYTES / 4
	);
	let _ = writeln!(
		inner_code,
		"#define PROGPOW_CNT_DAG         {}",
		PROGPOW_CNT_DAG
	);
	let _ = writeln!(
		inner_code,
		"#define PROGPOW_CNT_MATH        {}",
		P::CNT_MATH
	);
	let _ = writeln!(
		inner_code,
		"#define PROGPOW_DAG_ELEMENTS    {}",
		dag_elements
	);
	inner_code.push_str("typedef struct {uint s[PROGPOW_DAG_LOADS];} dag_t;\n\n");

	inner_code.push_str("typedef struct { uint z, w, jsr, jcong; } kiss99_t;\n\n");

	let kiss99_logic = if is_zano {
		r#"
    st->jcong = 69069 * st->jcong + 1234567;
    st->jsr ^= (st->jsr << 13);
    st->jsr ^= (st->jsr >> 17);
    st->jsr ^= (st->jsr << 5);
"#
	} else {
		r#"
    st->jsr ^= (st->jsr << 17);
    st->jsr ^= (st->jsr >> 13);
    st->jsr ^= (st->jsr << 5);
    st->jcong = 69069 * st->jcong + 1234567;
"#
	};

	let _ = writeln!(
		inner_code,
		"uint kiss99(kiss99_t *st) {{
    st->z = 36969 * (st->z & 65535) + (st->z >> 16);
    st->w = 18000 * (st->w & 65535) + (st->w >> 16);
    uint MWC = ((st->z << 16) + st->w);
{}
    return (MWC ^ st->jcong) + st->jsr;
}}",
		kiss99_logic
	);

	inner_code.push_str("void progPowLoop(const uint loop_cnt, uint mix[PROGPOW_REGS], __global const dag_t *g_dag, __local const uint *c_dag) {\n");
	inner_code.push_str("    dag_t data_dag;\n    uint offset, data;\n    const uint lane_id = get_local_id(0) & (PROGPOW_LANES-1);\n");

	// Global Load (OpenCL specific)
	inner_code.push_str("    offset = sub_group_broadcast(mix[0], loop_cnt % PROGPOW_LANES);\n");
	inner_code.push_str("    offset %= PROGPOW_DAG_ELEMENTS;\n");
	inner_code
		.push_str("    offset = offset * PROGPOW_LANES + (lane_id ^ loop_cnt) % PROGPOW_LANES;\n");
	inner_code.push_str("    data_dag = g_dag[offset];\n");

	// Math Generation (Identical logic, different formatting helper if needed, but C/OpenCL is close enough for math)
	// We reuse the 'math' and 'merge' functions but ensure they output valid OpenCL.
	// 'math' uses standard C ops which OpenCL supports. 'rotate' vs 'rotl32' macro handles difference.

	let max_ops = std::cmp::max(P::CNT_CACHE, P::CNT_MATH);
	for i in 0..max_ops {
		if i < P::CNT_CACHE {
			let src = format!("mix[{}]", mix_seq_cache[mix_seq_cache_cnt % PROGPOW_REGS]);
			mix_seq_cache_cnt += 1;
			let dest = format!("mix[{}]", mix_seq_dst[mix_seq_dst_cnt % PROGPOW_REGS]);
			mix_seq_dst_cnt += 1;
			let r = rng.rnd(is_zano);
			let _ = writeln!(inner_code, "    offset = {} % PROGPOW_CACHE_WORDS;", src);
			let _ = writeln!(inner_code, "    data = c_dag[offset];");
			inner_code.push_str(&merge(&dest, "data", r)); // merge is safe for OpenCL (macros handle rot)
		}
		if i < P::CNT_MATH {
			let src_rnd = (rng.rnd(is_zano) as usize) % ((PROGPOW_REGS - 1) * PROGPOW_REGS);
			let src1 = src_rnd % PROGPOW_REGS;
			let mut src2 = src_rnd / PROGPOW_REGS;
			if src2 >= src1 {
				src2 += 1;
			}
			let r1 = rng.rnd(is_zano);
			let dest = format!("mix[{}]", mix_seq_dst[mix_seq_dst_cnt % PROGPOW_REGS]);
			mix_seq_dst_cnt += 1;
			let r2 = rng.rnd(is_zano);
			inner_code.push_str(&math(
				"data",
				&format!("mix[{}]", src1),
				&format!("mix[{}]", src2),
				r1,
				P::MATH_MAPPING,
			));
			inner_code.push_str(&merge(&dest, "data", r2));
		}
	}

	// DAG Loads
	inner_code.push_str(&merge("mix[0]", "data_dag.s[0]", rng.rnd(is_zano)));
	for i in 1..PROGPOW_DAG_LOADS {
		let dest = format!("mix[{}]", mix_seq_dst[mix_seq_dst_cnt % PROGPOW_REGS]);
		mix_seq_dst_cnt += 1;
		let r = rng.rnd(is_zano);
		inner_code.push_str(&merge(&dest, &format!("data_dag.s[{}]", i), r));
	}

	inner_code.push_str("}\n\n");
	let mut final_source = String::from(STATIC_OPENCL_KERNEL_SOURCE);

	// Inject KAWPOW_IS_RAVENCOIN for OpenCL
	let opencl_defines = format!(
		"#define KAWPOW_IS_RAVENCOIN     {}\n#define XMRIG_INCLUDE_KECCAK_ROUNDS {}\n",
		if P::HAS_RAVENCOIN_RNDC { 1 } else { 0 },
		P::KECCAK_ROUNDS
	);

	final_source = final_source.replace(
		"#ifndef SEARCH_RESULTS",
		&format!("{}\n#ifndef SEARCH_RESULTS", opencl_defines),
	);

	inner_code.push_str(&final_source); // This footer is valid OpenCL

	inner_code
}

// --- TEMPLATES ---

const PROGPOW_KERNEL_TEMPLATE: &str = r#"
typedef unsigned int       uint32_t;
typedef unsigned long long uint64_t;

#ifndef SEARCH_RESULTS
#define SEARCH_RESULTS 16
#endif

typedef struct {
        uint64_t nonce;
        uint32_t mix[8];
        uint32_t debug[8];
    } search_result;

typedef struct {
    uint32_t count;
    uint32_t _padding; // Explicitly match Rust struct alignment
    search_result result[SEARCH_RESULTS];
} search_results;

#if __CUDA_ARCH__ < 350
    #define ROTL32(x,n) (((x) << (n % 32)) | ((x) >> (32 - (n % 32))))
    #define ROTR32(x,n) (((x) >> (n % 32)) | ((x) << (32 - (n % 32))))
#else
    #define ROTL32(x,n) __funnelshift_l((x), (x), (n))
    #define ROTR32(x,n) __funnelshift_r((x), (x), (n))
#endif

#define min(a,b)     ((a<b) ? a : b)
#define mul_hi(a, b) __umulhi(a, b)
#define clz(a)       __clz(a)
#define popcount(a)  __popc(a)

#define DEV_INLINE __device__ __forceinline__

#if (__CUDACC_VER_MAJOR__ > 8)
    #define SHFL(x, y, z) __shfl_sync(0xFFFFFFFF, (x), (y), (z))
#else
    #define SHFL(x, y, z) __shfl((x), (y), (z))
#endif

#define PROGPOW_LANES           16
#define PROGPOW_REGS            32
#define PROGPOW_DAG_LOADS       4
#define PROGPOW_CACHE_WORDS     4096
#define PROGPOW_CNT_DAG         64
XMRIG_INCLUDE_DEFINES

typedef struct __align__(16) {uint32_t s[PROGPOW_DAG_LOADS];} dag_t;

DEV_INLINE void progPowLoop(const uint32_t loop, uint32_t mix[PROGPOW_REGS], const dag_t *g_dag, const uint32_t c_dag[PROGPOW_CACHE_WORDS], const bool hack_false)
{
    dag_t data_dag;
    uint32_t offset, data;
    const uint32_t lane_id = threadIdx.x & (PROGPOW_LANES-1);

    // global load
    offset = SHFL(mix[0], loop % PROGPOW_LANES, PROGPOW_LANES);

    // OFFSET MOD LOGIC
    XMRIG_INCLUDE_OFFSET_MOD_DAG_ELEMENTS

    offset = offset * PROGPOW_LANES + (lane_id ^ loop) % PROGPOW_LANES;
    data_dag = g_dag[offset];

    if (hack_false) __threadfence_block();

    // Random math and cache operations
    XMRIG_INCLUDE_PROGPOW_RANDOM_MATH

    // DAG data loads (merge data_dag into mix)
    XMRIG_INCLUDE_PROGPOW_DATA_LOADS
}

#define FNV_PRIME 0x1000193
#define FNV_OFFSET_BASIS 0x811c9dc5

typedef struct
{
    uint32_t uint32s[32 / sizeof(uint32_t)];
} hash32_t;

__device__ __constant__ const uint32_t keccakf_rndc[24] = {
    0x00000001, 0x00008082, 0x0000808a, 0x80008000, 0x0000808b, 0x80000001,
    0x80008081, 0x00008009, 0x0000008a, 0x00000088, 0x80008009, 0x8000000a,
    0x8000808b, 0x0000008b, 0x00008089, 0x00008003, 0x00008002, 0x00000080,
    0x0000800a, 0x8000000a, 0x80008081, 0x00008080, 0x80000001, 0x80008008
};



__device__ __constant__ const uint32_t ravencoin_rndc[15] = {
        0x00000072, 0x00000041, 0x00000056, 0x00000045, 0x0000004E,
        0x00000043, 0x0000004F, 0x00000049, 0x0000004E,
        0x0000004B, 0x00000041, 0x00000057,
        0x00000050, 0x0000004F, 0x00000057
};




    // Theta
    __device__ __forceinline__ void keccak_f800_round(uint32_t* st, const int r)
    {

    // Theta
    uint32_t bc0 = st[0] ^ st[5] ^ st[10] ^ st[15] ^ st[20];
    uint32_t bc1 = st[1] ^ st[6] ^ st[11] ^ st[16] ^ st[21];
    uint32_t bc2 = st[2] ^ st[7] ^ st[12] ^ st[17] ^ st[22];
    uint32_t bc3 = st[3] ^ st[8] ^ st[13] ^ st[18] ^ st[23];
    uint32_t bc4 = st[4] ^ st[9] ^ st[14] ^ st[19] ^ st[24];

    uint32_t t0 = bc4 ^ ROTL32(bc1, 1);
    uint32_t t1 = bc0 ^ ROTL32(bc2, 1);
    uint32_t t2 = bc1 ^ ROTL32(bc3, 1);
    uint32_t t3 = bc2 ^ ROTL32(bc4, 1);
    uint32_t t4 = bc3 ^ ROTL32(bc0, 1);

    st[0] ^= t0;
    st[5] ^= t0;
    st[10] ^= t0;
    st[15] ^= t0;
    st[20] ^= t0;

    st[1] ^= t1;
    st[6] ^= t1;
    st[11] ^= t1;
    st[16] ^= t1;
    st[21] ^= t1;

    st[2] ^= t2;
    st[7] ^= t2;
    st[12] ^= t2;
    st[17] ^= t2;
    st[22] ^= t2;

    st[3] ^= t3;
    st[8] ^= t3;
    st[13] ^= t3;
    st[18] ^= t3;
    st[23] ^= t3;

    st[4] ^= t4;
    st[9] ^= t4;
    st[14] ^= t4;
    st[19] ^= t4;
    st[24] ^= t4;

    // Rho Pi
    uint32_t t = st[1];
    uint32_t tmp;

    tmp = st[10]; st[10] = ROTL32(t, 1); t = tmp;
    tmp = st[7];  st[7]  = ROTL32(t, 3); t = tmp;
    tmp = st[11]; st[11] = ROTL32(t, 6); t = tmp;
    tmp = st[17]; st[17] = ROTL32(t, 10); t = tmp;
    tmp = st[18]; st[18] = ROTL32(t, 15); t = tmp;
    tmp = st[3];  st[3]  = ROTL32(t, 21); t = tmp;
    tmp = st[5];  st[5]  = ROTL32(t, 28); t = tmp;
    tmp = st[16]; st[16] = ROTL32(t, 36); t = tmp;
    tmp = st[8];  st[8]  = ROTL32(t, 45); t = tmp;
    tmp = st[21]; st[21] = ROTL32(t, 55); t = tmp;
    tmp = st[24]; st[24] = ROTL32(t, 2); t = tmp;
    tmp = st[4];  st[4]  = ROTL32(t, 14); t = tmp;
    tmp = st[15]; st[15] = ROTL32(t, 27); t = tmp;
    tmp = st[23]; st[23] = ROTL32(t, 41); t = tmp;
    tmp = st[19]; st[19] = ROTL32(t, 56); t = tmp;
    tmp = st[13]; st[13] = ROTL32(t, 8); t = tmp;
    tmp = st[12]; st[12] = ROTL32(t, 25); t = tmp;
    tmp = st[2];  st[2]  = ROTL32(t, 43); t = tmp;
    tmp = st[20]; st[20] = ROTL32(t, 62); t = tmp;
    tmp = st[14]; st[14] = ROTL32(t, 18); t = tmp;
    tmp = st[22]; st[22] = ROTL32(t, 39); t = tmp;
    tmp = st[9];  st[9]  = ROTL32(t, 61); t = tmp;
    tmp = st[6];  st[6]  = ROTL32(t, 20); t = tmp;
    st[1] = ROTL32(t, 44);

    // Chi
    uint32_t c0, c1, c2, c3, c4;
    c0 = st[0]; c1 = st[1]; c2 = st[2]; c3 = st[3]; c4 = st[4];
    st[0] ^= (~c1) & c2;
    st[1] ^= (~c2) & c3;
    st[2] ^= (~c3) & c4;
    st[3] ^= (~c4) & c0;
    st[4] ^= (~c0) & c1;

    c0 = st[5]; c1 = st[6]; c2 = st[7]; c3 = st[8]; c4 = st[9];
    st[5] ^= (~c1) & c2;
    st[6] ^= (~c2) & c3;
    st[7] ^= (~c3) & c4;
    st[8] ^= (~c4) & c0;
    st[9] ^= (~c0) & c1;

    c0 = st[10]; c1 = st[11]; c2 = st[12]; c3 = st[13]; c4 = st[14];
    st[10] ^= (~c1) & c2;
    st[11] ^= (~c2) & c3;
    st[12] ^= (~c3) & c4;
    st[13] ^= (~c4) & c0;
    st[14] ^= (~c0) & c1;

    c0 = st[15]; c1 = st[16]; c2 = st[17]; c3 = st[18]; c4 = st[19];
    st[15] ^= (~c1) & c2;
    st[16] ^= (~c2) & c3;
    st[17] ^= (~c3) & c4;
    st[18] ^= (~c4) & c0;
    st[19] ^= (~c0) & c1;

    c0 = st[20]; c1 = st[21]; c2 = st[22]; c3 = st[23]; c4 = st[24];
    st[20] ^= (~c1) & c2;
    st[21] ^= (~c2) & c3;
    st[22] ^= (~c3) & c4;
    st[23] ^= (~c4) & c0;
    st[24] ^= (~c0) & c1;

    // Iota
    st[0] ^= keccakf_rndc[r];
}

__device__ __forceinline__ uint32_t cuda_swab32(const uint32_t x)
{
    // Explicit byte swap using shifts to ensure correctness on all archs
    return ((x & 0x000000FF) << 24) |
           ((x & 0x0000FF00) << 8)  |
           ((x & 0x00FF0000) >> 8)  |
           ((x & 0xFF000000) >> 24);
}

__device__ __forceinline__ void keccak_f800(uint32_t* st)
{
    #pragma unroll
    for (int r = 0; r < XMRIG_INCLUDE_KECCAK_ROUNDS; r++) {
        keccak_f800_round(st, r);
        if (r == 0) {
            // Forensic: Capture Round 0 state for gid 0 to verify alignment
            for (int i = 0; i < 25; i++) st[i] = st[i]; // No-op to allow breakpoint or just tracing if needed
        }
    }
}

__device__ __forceinline__ uint32_t fnv1a_dev(uint32_t h, uint32_t d)
{
    return (h ^ d) * FNV_PRIME;
}

typedef struct {
    uint32_t z, w, jsr, jcong;
} kiss99_t;

__device__ __forceinline__ uint32_t kiss99(kiss99_t &st)
{
    st.z = 36969 * (st.z & 65535) + (st.z >> 16);
    st.w = 18000 * (st.w & 65535) + (st.w >> 16);
    uint32_t MWC = ((st.z << 16) + st.w);

XMRIG_INCLUDE_KISS99_LOGIC

    uint32_t res = ((MWC^st.jcong) + st.jsr);
    return res;
}

__device__ __forceinline__ void fill_mix(uint32_t* hash_seed, uint32_t lane_id, uint32_t* mix, uint32_t* g_debug_trace)
{
    uint32_t fnv_hash = FNV_OFFSET_BASIS;
    kiss99_t st;
    st.z = fnv1a_dev(fnv_hash, hash_seed[0]);
    st.w = fnv1a_dev(st.z, hash_seed[1]);
    st.jsr = fnv1a_dev(st.w, lane_id);
    st.jcong = fnv1a_dev(st.jsr, lane_id);
    if (lane_id == 0 && (blockIdx.x * blockDim.x + threadIdx.x) == 0) {
        if (g_debug_trace != NULL) {
             g_debug_trace[210] = st.z;
             g_debug_trace[211] = st.w;
             g_debug_trace[212] = st.jsr;
             g_debug_trace[213] = st.jcong;
        }
    }
    // Iteration 0
    mix[0] = kiss99(st);
    if (lane_id == 0 && (blockIdx.x * blockDim.x + threadIdx.x) == 0 && g_debug_trace != NULL) {
         g_debug_trace[220] = st.z;
         g_debug_trace[221] = st.w;
         g_debug_trace[222] = st.jsr;
         g_debug_trace[223] = st.jcong;
         g_debug_trace[224] = mix[0];
    }
    #pragma unroll
    for (int i = 1; i < PROGPOW_REGS; i++)
        mix[i] = kiss99(st);
}

__device__ __forceinline__ bool u64_le(uint64_t a, uint64_t b)
{
    uint32_t a_hi = (uint32_t)(a >> 32);
    uint32_t b_hi = (uint32_t)(b >> 32);
    if (a_hi < b_hi) return true;
    if (a_hi > b_hi) return false;
    return (uint32_t)a <= (uint32_t)b;
}

extern "C" __global__ void progpow_search_v3(
    const uint64_t start_nonce,
    const uint64_t target,
    const uint64_t h0_64, const uint64_t h1_64, const uint64_t h2_64, const uint64_t h3_64,
    const dag_t* g_dag,
    const uint32_t* c_cache,
    volatile search_results* g_output,
    uint32_t* g_debug_trace
    )
{
    // Unpack 4x u64 into 8x u32
    const uint32_t header_hash[8] = {
        (uint32_t)h0_64, (uint32_t)(h0_64 >> 32),
        (uint32_t)h1_64, (uint32_t)(h1_64 >> 32),
        (uint32_t)h2_64, (uint32_t)(h2_64 >> 32),
        (uint32_t)h3_64, (uint32_t)(h3_64 >> 32)
    };
    // const uint32_t header_hash[8] = {0, 0, 0, 0, 0, 0, 0, 0};
    // const uint32_t* job_blob = (const uint32_t*)args.job_blob_addr; // No longer needed

    const bool hack_false = false;
    __shared__ uint32_t c_dag[PROGPOW_CACHE_WORDS];

    const uint32_t gid = blockIdx.x * blockDim.x + threadIdx.x;
    const uint32_t lane_id = gid & (PROGPOW_LANES - 1);
    const uint32_t nonce_id = gid / PROGPOW_LANES;

    // Load Cache
    for (uint32_t word = threadIdx.x; word < PROGPOW_CACHE_WORDS; word += blockDim.x)
    {
        c_dag[word] = c_cache[word];
    }
    __syncthreads();

    uint64_t nonce = start_nonce + nonce_id;

    uint32_t mix[PROGPOW_REGS];
    uint32_t hash_seed[4];
    uint32_t state2[8];

    // Debug: Dump kernel arguments (gid 0)
    if (gid == 0 && g_debug_trace != NULL) {
        // 500: Header (h0_64) - First 8 bytes instead of pointer
        g_debug_trace[500] = (uint32_t)h0_64;
        g_debug_trace[501] = (uint32_t)(h0_64 >> 32);

        // 502: DAG Ptr
        uint64_t dag_val = (uint64_t)g_dag;
        g_debug_trace[502] = (uint32_t)dag_val;
        g_debug_trace[503] = (uint32_t)(dag_val >> 32);

        // 504: Cache Ptr
        uint64_t cache_val = (uint64_t)c_cache;
        g_debug_trace[504] = (uint32_t)cache_val;
        g_debug_trace[505] = (uint32_t)(cache_val >> 32);

        // 506: Start Nonce
        g_debug_trace[506] = (uint32_t)start_nonce;
        g_debug_trace[507] = (uint32_t)(start_nonce >> 32);

        // 508: Target
        g_debug_trace[508] = (uint32_t)target;
        g_debug_trace[509] = (uint32_t)(target >> 32);

        // 514: Output Ptr
        uint64_t out_val = (uint64_t)g_output;
        g_debug_trace[514] = (uint32_t)out_val;
        g_debug_trace[515] = (uint32_t)(out_val >> 32);

        // 516: Debug Ptr
        uint64_t dbg_val = (uint64_t)g_debug_trace;
        g_debug_trace[516] = (uint32_t)dbg_val;
        g_debug_trace[517] = (uint32_t)(dbg_val >> 32);
    }

    {
        // Initial state
        uint32_t state[25];
        for (int i = 0; i < 25; i++) state[i] = 0;

        for (int i = 0; i < 8; i++)
            state[i] = header_hash[i];

        state[8] = (uint32_t)nonce;
        state[9] = (uint32_t)(nonce >> 32);

        if (gid == 0 && g_debug_trace != NULL) {
             // for(int i=0; i<8; i++) g_debug_trace[100+i] = state[i]; // Trace initial state (header)
        }


XMRIG_INCLUDE_PROGPOW_INITIAL_PADDING

        keccak_f800(state);

        for (int i = 0; i < 8; i++)
            state2[i] = state[i];

        uint32_t hash_seed_small[2];
        XMRIG_INCLUDE_HASH_SEED_EXTRACT
        hash_seed[0] = hash_seed_small[0];
        hash_seed[1] = hash_seed_small[1];
    }
    if (gid == 0) {
        if (g_debug_trace != NULL) {
            g_debug_trace[200] = hash_seed[0];
            g_debug_trace[201] = hash_seed[1];
        }
    }
    fill_mix(hash_seed, lane_id, mix, g_debug_trace);

    if (gid == 0 && g_debug_trace != NULL) {
        // Trace Mix Init (Offset 32)
        for(int i=0; i<8; i++) g_debug_trace[32+i] = mix[i]; // Store lane 0 mix
    }

    #pragma unroll 1
    for (uint32_t l = 0; l < PROGPOW_CNT_DAG; l++) {
        progPowLoop(l, mix, g_dag, c_dag, hack_false);
        if (gid == 0 && l == 0) {
             if (g_debug_trace != NULL) {
                 // Trace Mix Loop 0 (Offset 48)
                 for(int i=0; i<8; i++) g_debug_trace[48+i] = mix[i];
             }
        }
    }


    // Reduction
    uint32_t digest_lane = FNV_OFFSET_BASIS;
    #pragma unroll
    for (int i = 0; i < PROGPOW_REGS; i++)
        digest_lane = fnv1a_dev(digest_lane, mix[i]);

    hash32_t digest;
    for (int i = 0; i < 8; i++)
    {
        uint32_t res = FNV_OFFSET_BASIS;
        res = fnv1a_dev(res, SHFL(digest_lane, i, PROGPOW_LANES));
        res = fnv1a_dev(res, SHFL(digest_lane, i + 8, PROGPOW_LANES));
        digest.uint32s[i] = res;
    }

    uint64_t result;
    {
        uint32_t final_state[25];
        for (int i = 0; i < 25; i++) final_state[i] = 0;

#if KAWPOW_IS_RAVENCOIN
        for (int i = 0; i < 8; i++)
            final_state[i] = state2[i];
        for (int i = 8; i < 16; i++)
            final_state[i] = digest.uint32s[i - 8];
        for (int i = 16; i < 25; i++)
            final_state[i] = ravencoin_rndc[i - 16]; // Corrected: Words 16-24 of state = Padding words 0-8
#else
        // Zano / Standard ProgPow
        for (int i = 0; i < 8; i++) final_state[i] = header_hash[i];

#if PROGPOW_IS_ZANO
        // Zano: seed is bswap64 of state2 - same transformation as hash_seed
        // Reference: zano keccak_progpow_256(header, seed, mix)
        final_state[8] = cuda_swab32(state2[1]);
        final_state[9] = cuda_swab32(state2[0]);
#else
        // Standard ProgPow uses nonce (or state2 directly)
        final_state[8] = state2[0];
        final_state[9] = state2[1];
#endif

        for (int i = 10; i < 18; i++) final_state[i] = digest.uint32s[i - 10];
#endif

        keccak_f800(final_state);
        // KawPoW: The 64-bit result for target comparison is the first 8 bytes of the hash
        // as a big-endian integer to match CPU verifier.
        result = ((uint64_t)cuda_swab32(final_state[0]) << 32) | (uint64_t)cuda_swab32(final_state[1]);

        if (gid == 0 && g_debug_trace != NULL) {
             // Optional trace logic can stay if guarded by explicit non-null check, but user asked to chill output.
             // We'll keep the g_debug_trace writes as they are silent, but remove printf.
             for(int i=0; i<25; i++) g_debug_trace[64+i] = final_state[i];
             g_debug_trace[90] = (uint32_t)(result >> 32);
             g_debug_trace[91] = (uint32_t)result;
             g_debug_trace[92] = (uint32_t)(target >> 32);
             g_debug_trace[93] = (uint32_t)target;
         }
    }

     if (u64_le(result, target) && result > 0 && lane_id == 0)
     {
         uint32_t index = atomicAdd((uint32_t*)&g_output->count, 1);
         if (index < SEARCH_RESULTS)
         {
             g_output->result[index].nonce = nonce;
             for (int i = 0; i < 8; i++) g_output->result[index].mix[i] = digest.uint32s[i];

            for (int i = 0; i < 8; i++) g_output->result[index].debug[i] = state2[i];
        }
    }
}
"#;

const STATIC_OPENCL_KERNEL_SOURCE: &str = r#"
#ifndef SEARCH_RESULTS
#define SEARCH_RESULTS 4
#endif

typedef struct {
    uint count;
    struct {
        ulong nonce;
        uint mix[8];
        uint debug[8];
    } result[SEARCH_RESULTS];
} search_results;

typedef struct {
    uint uint32s[8];
} hash32_t;

__constant uint keccakf_rndc[24] = {
    0x00000001, 0x00008082, 0x0000808a, 0x80008000, 0x0000808b, 0x80000001,
    0x80008081, 0x00008009, 0x0000008a, 0x00000088, 0x80008009, 0x8000000a,
    0x8000808b, 0x0000008b, 0x00008089, 0x00008003, 0x00008002, 0x00000080,
    0x0000800a, 0x8000000a, 0x80008081, 0x00008080, 0x80000001, 0x80008008
};

__constant uint ravencoin_kawpow[15] = {
    0x00000072, 0x00000041, 0x00000056, 0x00000045, 0x0000004E, // RAVEN
    0x00000043, 0x0000004F, 0x00000049, 0x0000004E,             // COIN
    0x0000004B, 0x00000041, 0x00000057,                         // KAW
    0x00000050, 0x0000004F, 0x00000057                          // POW
};

inline void keccak_f800_round(uint st[25], const int r)
{
    const uint keccakf_rotc[24] = {
        1,  3,  6,  10, 15, 21, 28, 4,  13, 23, 2,  14,
        27, 9,  24, 8,  25, 11, 30, 18, 7,  29, 20, 12
    };
    const uint keccakf_piln[24] = {
        10, 7,  11, 17, 18, 3, 5,  16, 8,  21, 24, 4,
        15, 23, 19, 13, 12, 2, 20, 14, 22, 9,  6,  1
    };
    uint t, bc[5];
    // Theta
    for (int i = 0; i < 5; i++)
        bc[i] = st[i] ^ st[i + 5] ^ st[i + 10] ^ st[i + 15] ^ st[i + 20];

    for (int i = 0; i < 5; i++) {
        t = bc[(i + 4) % 5] ^ ROTL32(bc[(i + 1) % 5], 1);
        for (uint j = 0; j < 25; j += 5)
            st[j + i] ^= t;
    }
    // Rho Pi
    t = st[1];
    for (int i = 0; i < 24; i++) {
        uint j = keccakf_piln[i];
        bc[0] = st[j];
        st[j] = ROTL32(t, keccakf_rotc[i]);
        t = bc[0];
    }
    // Chi
    for (int j = 0; j < 25; j += 5) {
        for (int i = 0; i < 5; i++)
            bc[i] = st[j + i];
        for (int i = 0; i < 5; i++)
            st[j + i] ^= (~bc[(i + 1) % 5]) & bc[(i + 2) % 5];
    }
    // Iota
    st[0] ^= keccakf_rndc[r];
}

inline void keccak_f800(uint st[25])
{
    for (int i = 0; i < XMRIG_INCLUDE_KECCAK_ROUNDS; i++)
        keccak_f800_round(st, i);
}

#define fnv1(h, d) (h = (uint(h) * uint(0x1000193)) ^ uint(d))
#define fnv1a(h, d) (h = (uint(h) ^ uint(d)) * uint(0x1000193))

typedef struct {
    uint z, w, jsr, jcong;
} kiss99_t;

inline uint kiss99(kiss99_t *st) {
    st->z = 36969 * (st->z & 65535) + (st->z >> 16);
    st->w = 18000 * (st->w & 65535) + (st->w >> 16);
    uint mwc = (st->z << 16) + st->w;
    st->jsr ^= st->jsr << 17;
    st->jsr ^= st->jsr >> 13;
    st->jsr ^= st->jsr << 5;
    st->jcong = 69069 * st->jcong + 1234567;
    return (mwc ^ st->jcong) + st->jsr;
}

void fill_mix(uint hash_seed[2], uint lane_id, uint mix[PROGPOW_REGS])
{
    kiss99_t st;
    st.z = (0x811c9dc5u ^ hash_seed[0]) * 0x1000193u;
    st.w = (st.z ^ hash_seed[1]) * 0x1000193u;
    st.jsr = (st.w ^ lane_id) * 0x1000193u;
    st.jcong = (st.jsr ^ lane_id) * 0x1000193u;

    for (int i = 0; i < PROGPOW_REGS; i++)
        mix[i] = kiss99(&st);
}

__kernel void progpow_search(
    const ulong start_nonce,
    const ulong target,
    __global const hash32_t* header,
    __global const dag_t *g_dag,
    __global const uint *c_cache,
    __global volatile search_results* g_output,
    __global uint* g_debug_trace
    )
{
    __local uint c_dag[PROGPOW_CACHE_WORDS];
    uint const gid = get_global_id(0);
    ulong const nonce = start_nonce + (gid / PROGPOW_LANES);

    uint const lane_id = get_local_id(0) & (PROGPOW_LANES - 1);

    // Load light cache into local memory
    for (uint word = get_local_id(0); word < PROGPOW_CACHE_WORDS; word += get_local_size(0))
    {
        c_dag[word] = c_cache[word];
    }
    barrier(CLK_LOCAL_MEM_FENCE);

    uint hash_seed[2];
    hash32_t digest;
    uint state2[8];

    {
        uint state[25];
        for(int i=0; i<25; i++) state[i] = 0;

        for (int i = 0; i < 8; i++)
            state[i] = header->uint32s[i];

        if (gid == 0 && g_debug_trace != NULL) {
            // Write Initial State (Header) to debug buffer at offset 64
            for (int i = 0; i < 8; i++) g_debug_trace[64 + i] = state[i];

            // Dump Arguments to debug buffer
            // 80: Header Ptr
            uint64_t v;
            v = (uint64_t)header; g_debug_trace[80] = (uint32_t)v; g_debug_trace[81] = (uint32_t)(v>>32);
            // 82: DAG Ptr
            v = (uint64_t)g_dag; g_debug_trace[82] = (uint32_t)v; g_debug_trace[83] = (uint32_t)(v>>32);
            // 84: Cache Ptr
            v = (uint64_t)c_cache; g_debug_trace[84] = (uint32_t)v; g_debug_trace[85] = (uint32_t)(v>>32);
            // 86: Start Nonce
            v = start_nonce; g_debug_trace[86] = (uint32_t)v; g_debug_trace[87] = (uint32_t)(v>>32);
            // 88: Target
            v = target; g_debug_trace[88] = (uint32_t)v; g_debug_trace[89] = (uint32_t)(v>>32);

            printf("GPU Header Ptr: %p\n", header);

            // Also printf for immediate feedback
            printf("GPU Header: %08x %08x %08x %08x %08x %08x %08x %08x\n",
                state[0], state[1], state[2], state[3],
                state[4], state[5], state[6], state[7]);
        }

        state[8] = (uint)nonce;
        state[9] = (uint)(nonce >> 32);

        XMRIG_INCLUDE_PROGPOW_INITIAL_PADDING

        keccak_f800(state);

        for (int i = 0; i < 8; i++)
            state2[i] = state[i];
    }

    uint hash_seed[2];
    hash_seed[0] = state2[0];
    hash_seed[1] = state2[1];
    uint mix[PROGPOW_REGS];
    fill_mix(hash_seed, lane_id, mix);

    #pragma unroll 1
    for (uint l = 0; l < PROGPOW_CNT_DAG; l++)
        progPowLoop(l, mix, g_dag, c_dag);

    uint digest_lane = 0x811c9dc5u;
    for (int i = 0; i < PROGPOW_REGS; i++)
        digest_lane = (digest_lane ^ mix[i]) * 0x1000193u;

    hash32_t digest_temp;
    for (int i = 0; i < 8; i++)
        digest_temp.uint32s[i] = 0x811c9dc5;

    for (int i = 0; i < PROGPOW_LANES; i += 8)
        for (int j = 0; j < 8; j++) {
            uint val = sub_group_broadcast(digest_lane, i + j);
            digest_temp.uint32s[j] = (digest_temp.uint32s[j] ^ val) * 0x1000193u;
        }

    digest = digest_temp;

    ulong result;
    {
        uint state[25];
#if KAWPOW_IS_RAVENCOIN
        for (int i = 0; i < 8; i++)
            state[i] = state2[i];
        for (int i = 8; i < 16; i++)
            state[i] = digest.uint32s[i - 8];
        for (int i = 16; i < 25; i++)
            state[i] = ravencoin_rndc[i - 16];
#else
        // Zano Style Finalization
        for(int i=0; i<8; i++) state[i] = header_hash[i];
        state[8] = state2[0];
        state[9] = state2[1];

        for(int i=10; i<18; i++) state[i] = digest.uint32s[i-10];
#endif
        keccak_f800(state);

        // OpenCL Byte verification
        uint s0 = state[0];
        uint s1 = state[1];
        uint b0 = ((s0 >> 24) & 0xff) | ((s0 >> 8) & 0xff00) | ((s0 << 8) & 0xff0000) | ((s0 << 24) & 0xff000000);
        uint b1 = ((s1 >> 24) & 0xff) | ((s1 >> 8) & 0xff00) | ((s1 << 8) & 0xff0000) | ((s1 << 24) & 0xff000000);
        result = (ulong)b0 << 32 | b1;
    }

    if (result <= target)
    {
        uint32_t index = atomicAdd((uint32_t*)&g_output->count, 1);
        if (index < SEARCH_RESULTS)
        {
            g_output->result[index].nonce = nonce;
            for (int i = 0; i < 8; i++){
                g_output->result[index].mix[i] = digest.uint32s[i];
                g_output->result[index].debug[i] = state2[i];
            }
        }
    }
}
"#;
