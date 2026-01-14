# Build System Configuration Notes

## Overview

This project builds a Rust application that links with C++ and CUDA code. The build configuration must handle:
- Cross-platform compilation (Windows MSVC, Linux GCC/Clang, macOS)
- Multiple build profiles (debug, release, custom cargo profiles)
- Rust FFI compatibility with C/C++ runtime libraries
- CUDA architecture targeting for optimal mining performance

## Debug vs Release Builds - CRITICAL UNDERSTANDING

### Release Builds (Production)
```
Rust flags: -O3 equivalent (full optimization)
C++ flags:  /MD /O2 /Ob2 /DNDEBUG (MSVC) or -O3 (GCC/Clang)
Runtime:    Release C runtime library
Goal:       Maximum performance, small binary, shipped to users
```

### Debug Builds (Development)
```
Rust flags: No optimization, debug assertions enabled
C++ flags:  /MD /Zi /Od /RTC1 (MSVC) or -g -O0 (GCC/Clang)
Runtime:    Release C runtime (/MD) - NOT debug runtime (/MDd)
Goal:       Debuggable code, runtime checks, fast compile times
```

### Why Always /MD (Not /MDd)?

**The Problem:**
- Rust's MSVC target **always** links against the **release** C runtime (`msvcrt.lib` / `/MD`)
- This is true even for `cargo build` (debug builds)
- If C++ code uses `/MDd` (debug runtime), the linker finds two different C runtimes:
  - Rust code: `msvcrt.lib` (release)
  - C++ code: `msvcrtd.lib` (debug)
- Result: Linker errors like `LNK2038: mismatch detected for '_ITERATOR_DEBUG_LEVEL'`

**The Solution:**
- C++ code must **always** use `/MD` (release runtime), even in debug builds
- We still get debug symbols (`/Zi`), runtime checks (`/RTC1`), and can debug C++ code
- We lose some debug heap checks from the debug C runtime, but that's acceptable
- This is documented in Rust's FFI guidelines and is standard practice

## MSVC Runtime Flag Management

### Centralized in build.rs (PRIMARY SOURCE OF TRUTH)

All runtime library flags (`/MD`, `/MDd`) are set in `build.rs`:
```rust
if is_release {
    make.define("CMAKE_CXX_FLAGS_RELEASE", "/MD /O2 /Ob2 /DNDEBUG /GL-");
} else {
    make.define("CMAKE_CXX_FLAGS_DEBUG", "/MD /Zi /Ob0 /Od /RTC1 /GL-");
}
```

**Why in build.rs?**
- Single source of truth prevents conflicts
- Cargo knows the actual build profile
- CMake generator expressions can evaluate at unpredictable times
- Easier to maintain and debug

**Exception:** CUDA compiler flags in `CMakeLists.txt` use `-Xcompiler=/MD` because NVCC needs explicit host compiler flags.

### Link Time Code Generation (/GL) - DISABLED

**What is /GL?**
- Link Time Code Generation (LTCG) optimizes across translation units
- Can provide 5-15% performance boost in C++ code
- Requires all libraries to be compiled with /GL

**Why is it disabled?**
- Rust's FFI boundaries expect standard calling conventions
- `/GL` can change calling conventions and inline across DLL boundaries
- This breaks `extern "C"` ABI guarantees
- Symptoms: Crashes, wrong parameters, corrupted stack
- **Decision:** Disable /GL (`/GL-`) for Rust FFI compatibility

## CUDA Architecture Configuration

### Current Targets (CMAKE_CUDA_ARCHITECTURES)
```cmake
set(CMAKE_CUDA_ARCHITECTURES "86;89;90;120")
```

| Arch | GPU Series | Examples | Mining Relevance |
|------|------------|----------|------------------|
| sm_86 | Ampere (RTX 30) | 3060, 3070, 3080, 3090 | Most common mining GPUs |
| sm_89 | Ada Lovelace (RTX 40) | 4060, 4070, 4080, 4090 | Current gen, excellent efficiency |
| sm_90 | Hopper | H100 | Datacenter, rare but powerful |
| sm_120 | Blackwell (RTX 50) | 5090, 5080, B200 | Latest generation (2025+) |

### Excluded Architectures
- **sm_61** (Pascal: GTX 1080): Available in test rig, use OpenCL/WGPU fallback
- **sm_75** (Turing: RTX 2060/2070/2080): Less common in mining, OpenCL covers it
- **Older** (Maxwell, Kepler): Too old, poor mining performance, not worth build time

### Build Time Impact
- Each architecture adds ~2-3 minutes to CUDA compilation
- `--threads 0` enables parallel compilation (uses all CPU threads)
- Reduced from 15+ minutes to ~3-5 minutes with optimized arch list

### Adding/Removing Architectures
To support additional GPUs, add compute capability to the list:
```cmake
set(CMAKE_CUDA_ARCHITECTURES "86;89;90;120;75")  # Added sm_75
```

Check CUDA compatibility: https://developer.nvidia.com/cuda-gpus

## Cross-Platform Build Support

### Windows (MSVC)
- Detected via `cfg!(target_env = "msvc")` in build.rs
- Uses `/MD` runtime flags
- Requires Visual Studio 2022+ for CUDA 13.1

### Linux (GCC/Clang)
- Standard `-O2`/`-O3` optimization flags
- No special runtime library concerns (glibc is dynamically linked)
- CUDA works with GCC 11+ or Clang 14+

### macOS (Clang)
- Similar to Linux
- No CUDA support (Apple Silicon doesn't support NVIDIA)
- Metal compute shaders would be future work

### Cross-Compilation
The build system detects the **target** environment, not the host:
```rust
if cfg!(target_env = "msvc") {  // TRUE when compiling for Windows, even from Linux
```

This works correctly for cross-compilation scenarios (e.g., building Windows binaries on Linux).

## Custom Cargo Profiles

Cargo supports custom profiles beyond `dev` and `release`:
```toml
[profile.bench]
inherits = "release"

[profile.release-with-debug]
inherits = "release"
debug = true
```

**Handling in build.rs:**
```rust
let is_release = profile == "release" || profile == "release-with-debug";
```

- Explicitly check for `"release"` and variants
- Treat everything else (dev, test, bench, custom) as debug for CMake
- CMake only understands "Debug" and "Release", so we map cargo's rich profile system

## Troubleshooting

### Linker Error: `LNK2038: mismatch detected for '_ITERATOR_DEBUG_LEVEL'`
- Cause: Mixing `/MD` and `/MDd` runtimes
- Fix: Verify `build.rs` sets `/MD` for both debug and release

### CUDA Compilation Takes Too Long
- Check `CMAKE_CUDA_ARCHITECTURES` - remove unused architectures
- Verify `--threads 0` is set in CUDA compile options
- Consider ccache for incremental builds

### Wrong CMAKE_BUILD_TYPE
- Verify `PROFILE` env var in build.rs matches expected cargo profile
- Check CMake output: "Build files for Release" or "Debug"

### FFI Crashes or Corruption
- Verify `/GL` is disabled (`/GL-` flag present)
- Check all C++ libraries use `/MD` (not `/MT` or `/MDd`)
- Verify `extern "C"` on all Rust-callable functions

## Performance Considerations

### Optimization Priority (for mining):
1. **CUDA kernel performance** (95% of compute time)
   - Controlled by `--use_fast_math` and architecture targeting
2. **C++ host code** (4% of time)
   - Controlled by `/O2` optimization
3. **Rust code** (1% of time)
   - Standard Cargo release profile sufficient

### Why Not Full LTCG?
- LTCG on C++ side would provide ~5-10% improvement
- But mining performance is CUDA-bound (GPU, not CPU)
- C++ is just orchestration code
- Not worth the FFI compatibility risk

## Future Improvements

### Potential Optimizations
- [ ] Profile-guided optimization (PGO) for C++ code
- [ ] LTO on Rust side (doesn't affect C++ FFI)
- [ ] Benchmark different CUDA architectures vs build time tradeoff
- [ ] Consider separate builds for different GPU generations

### Maintainability
- [ ] CI/CD testing for all supported platforms
- [ ] Automated verification of runtime flag consistency
- [ ] CUDA architecture detection at runtime (dynamic dispatch)

## Summary of Key Decisions

| Decision | Rationale |
|----------|-----------|
| Always use /MD | Rust FFI compatibility requirement |
| Disable /GL | Prevents FFI ABI issues |
| Runtime flags in build.rs | Single source of truth |
| Limited CUDA architectures | Build time vs coverage tradeoff |
| --threads 0 | Parallel CUDA compilation |
| Release runtime in debug | Rust's default behavior |

---

**Last Updated:** 2026-01-14
**CUDA Version:** 13.1
**CMake Minimum:** 3.18
**Rust MSVC Target:** Always links against release runtime
