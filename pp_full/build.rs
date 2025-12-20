extern crate bindgen;
extern crate cmake;
extern crate filetime;

use filetime::FileTime;

use std::env;
use std::fs;

pub fn fail_on_empty_directory(name: &str) {
	if fs::read_dir(name).unwrap().count() == 0 {
		println!(
			"The `{}` directory is empty. Did you forget to pull the submodules?",
			name
		);
		println!("Try `git submodule update --init --recursive`");
		panic!();
	}
}

fn generate_bindings(out_dir: &str) {
	let bindings = bindgen::Builder::default()
		.header("lib/libexternal/progpow.h")
		.blocklist_type("max_align_t")
		.blocklist_type("_bindgen_ty_1")
		.generate()
		.expect("Unable to generate bindings");

	//let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
	bindings
		.write_to_file(format!("{}/ffi.rs", out_dir))
		.expect("Couldn't write bindings!");
}

fn compile_cmake() {
	let mut make = cmake::Config::new("lib");

	if cfg!(feature = "cuda") {
		make.define("ETHASHCUDA", "ON");
	} else {
		make.define("ETHASHCUDA", "OFF");
	}

	if cfg!(feature = "opencl") {
		make.define("ETHASHCL", "ON");
	} else {
		make.define("ETHASHCL", "OFF");
	}

	// On Windows, help CMake find Boost
	if cfg!(target_os = "windows") {
		// Check common Boost locations on Windows
		let boost_paths = vec![
			"C:\\local\\boost_1_78_0",
			"C:\\Boost",
			"C:\\Program Files\\boost",
		];

		for path in &boost_paths {
			if std::path::Path::new(path).exists() {
				println!("cargo:warning=Found Boost at {}", path);
				make.define("BOOST_ROOT", path);
				make.define("BOOST_INCLUDEDIR", path);
				make.define("Boost_NO_SYSTEM_PATHS", "ON");
				break;
			}
		}

		// Also check environment variables
		if let Ok(boost_root) = env::var("BOOST_ROOT") {
			println!("cargo:warning=Using BOOST_ROOT from environment: {}", boost_root);
			make.define("BOOST_ROOT", &boost_root);
			make.define("BOOST_INCLUDEDIR", &boost_root);
			make.define("Boost_NO_SYSTEM_PATHS", "ON");
		}

		// Add OpenCL include path for Windows
		if cfg!(feature = "opencl") {
			let opencl_include = "C:\\Program Files\\NVIDIA GPU Computing Toolkit\\CUDA\\v13.1\\include";
			if std::path::Path::new(opencl_include).exists() {
				println!("cargo:warning=Adding OpenCL include path: {}", opencl_include);
				make.cxxflag(format!("/I\"{}\"", opencl_include));
			}
		}
	}

	make.no_build_target(true).build();
}

fn exec_if_newer<F: Fn()>(inpath: &str, outpath: &str, build: F) {
	if let Ok(metadata) = fs::metadata(outpath) {
		let outtime = FileTime::from_last_modification_time(&metadata);
		let intime = FileTime::from_last_modification_time(
			&fs::metadata(inpath).expect(&format!("Path {} not found", inpath)),
		);
		let buildfiletime =
			FileTime::from_last_modification_time(&fs::metadata("build.rs").unwrap());
		if outtime > intime && outtime > buildfiletime {
			return;
		}
	}
	build();
}

fn main() {
	println!("Starting progpow build");

	let out_dir = env::var("OUT_DIR").unwrap();

	fail_on_empty_directory("lib");

	compile_cmake();

	if cfg!(target_env = "msvc") {
		let target = if cfg!(debug_assertions) {
			"Debug"
		} else {
			"Release"
		};

		if cfg!(feature = "opencl") {
			println!(
				"cargo:rustc-link-search={}/build/libethash-cl/{}",
				out_dir, target
			);
			println!("cargo:rustc-link-lib=ethash-cl");
		}

		if cfg!(feature = "cuda") {
			println!(
				"cargo:rustc-link-search={}/build/libethash-cuda/{}",
				out_dir, target
			);
			println!("cargo:rustc-link-lib=ethash-cuda");
			println!("cargo:rustc-link-lib=cudart");
			println!("cargo:rustc-link-lib=cuda");
			println!("cargo:rustc-link-lib=nvrtc");
		}

		println!(
			"cargo:rustc-link-search={}/build/libethash/{}",
			out_dir, target
		);
		println!("cargo:rustc-link-lib=ethash");
		println!(
			"cargo:rustc-link-search={}/build/libprogpow/{}",
			out_dir, target
		);
		println!("cargo:rustc-link-lib=progpow");
		println!(
			"cargo:rustc-link-search={}/build/libethcore/{}",
			out_dir, target
		);
		println!("cargo:rustc-link-lib=ethcore");
		println!(
			"cargo:rustc-link-search={}/build/libdevcore/{}",
			out_dir, target
		);
		println!("cargo:rustc-link-lib=devcore");
		println!(
			"cargo:rustc-link-search={}/build/libexternal/{}",
			out_dir, target
		);
		println!("cargo:rustc-link-lib=ppow");

		// Add CUDA lib path for OpenCL
		if let Ok(cuda_path) = env::var("CUDA_PATH") {
			println!("cargo:rustc-link-search={}/lib/x64", cuda_path);
		} else {
			println!("cargo:rustc-link-search=C:/Program Files/NVIDIA GPU Computing Toolkit/CUDA/v13.1/lib/x64");
		}

		println!("cargo:rustc-link-lib=OpenCL");
	} else {
		println!("cargo:rustc-link-search={}/build/libexternal", out_dir);
		println!("cargo:rustc-link-lib=ppow");
	}
}
