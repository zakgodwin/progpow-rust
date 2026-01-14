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

	// MSVC Runtime Configuration:
	// Rust always links against the release C runtime (/MD) even in debug builds.
	// C++ code MUST also use /MD (not /MDd) to avoid linker errors.
	// We still get debug symbols (/Zi) and can debug, but lose some C runtime debug checks.
	//
	// /GL (Link Time Code Generation) is DISABLED because it's incompatible with Rust's
	// FFI expectations. LTCG changes calling conventions and can break extern "C" boundaries.
	//
	// Profile detection handles custom cargo profiles (bench, dev, etc.) by treating
	// anything non-release as debug for CMake purposes.
	if cfg!(target_env = "msvc") {
		let profile = env::var("PROFILE").unwrap_or_else(|_| String::from("debug"));

		// Treat "release" as Release, everything else (debug, dev, test, bench) as Debug
		let is_release = profile == "release" || profile == "release-with-debug";

		if is_release {
			make.profile("Release");
			// Release: Full optimization, no debug symbols, always /MD runtime
			make.define("CMAKE_CXX_FLAGS_RELEASE", "/MD /O2 /Ob2 /DNDEBUG /GL-");
			make.define("CMAKE_C_FLAGS_RELEASE", "/MD /O2 /Ob2 /DNDEBUG /GL-");
		} else {
			make.profile("Debug");
			// Debug: No optimization, debug symbols, but STILL /MD (not /MDd) for Rust compatibility
			make.define("CMAKE_CXX_FLAGS_DEBUG", "/MD /Zi /Ob0 /Od /RTC1 /GL-");
			make.define("CMAKE_C_FLAGS_DEBUG", "/MD /Zi /Ob0 /Od /RTC1 /GL-");
		}
	}

	make.build_target("ppow_progpow").build();
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
			println!("cargo:rustc-link-lib=ethash-cuda-device");
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
		println!("cargo:rustc-link-lib=static=ppow_progpow");
		println!("cargo:rustc-link-lib=OpenCL");
	} else {
		println!("cargo:rustc-link-search={}/build/libexternal", out_dir);
		println!("cargo:rustc-link-lib=static=ppow_progpow");
	}
}


