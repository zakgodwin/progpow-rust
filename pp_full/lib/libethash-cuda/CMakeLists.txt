# Modern CMake CUDA library configuration for CUDA 13.1+ and Visual Studio 2022
# Requires CMake 3.18+ for first-class CUDA support
cmake_minimum_required(VERSION 3.18)

# Enable CUDA language support
enable_language(CUDA)

# Set CUDA standard
set(CMAKE_CUDA_STANDARD 14)
set(CMAKE_CUDA_STANDARD_REQUIRED ON)

# CUDA Target Architectures (compute capabilities):
# Mining performance is architecture-specific, so we compile for modern GPUs only.
# Older architectures fall back to OpenCL/WGPU for broader compatibility.
#
# sm_86  = Ampere (RTX 30-series: 3060, 3070, 3080, 3090)
# sm_89  = Ada Lovelace (RTX 40-series: 4060, 4070, 4080, 4090)
# sm_90  = Hopper (H100 datacenter GPUs)
# sm_120 = Blackwell (RTX 50-series and B-series datacenter - RTX 5090, etc.)
#
# Excluded older architectures to reduce build time (15+ min → ~3-5 min):
# - sm_61 (Pascal: GTX 1060/1070/1080) - Use for Linux test rig, covered by OpenCL
# - sm_75 (Turing: RTX 20-series) - Less common in mining, OpenCL fallback available
#
# --threads 0 = Use all available CPU threads for parallel CUDA compilation
set(CMAKE_CUDA_ARCHITECTURES "86;89;90;120")

# Generate CUDA kernel header file
add_custom_command(
	OUTPUT ${CMAKE_CURRENT_BINARY_DIR}/CUDAMiner_kernel.h
	COMMAND ${CMAKE_COMMAND} ARGS
	-DBIN2H_SOURCE_FILE=${CMAKE_CURRENT_SOURCE_DIR}/CUDAMiner_kernel.cu
	-DBIN2H_VARIABLE_NAME=CUDAMiner_kernel
	-DBIN2H_HEADER_FILE=${CMAKE_CURRENT_BINARY_DIR}/CUDAMiner_kernel.h
	-P ${CMAKE_CURRENT_SOURCE_DIR}/../bin2h.cmake
	COMMENT "Generating CUDA Kernel Byte Array"
	DEPENDS ${CMAKE_CURRENT_SOURCE_DIR}/CUDAMiner_kernel.cu
	VERBATIM
)
add_custom_target(cuda_kernel DEPENDS ${CMAKE_CURRENT_BINARY_DIR}/CUDAMiner_kernel.h)

# Split into Device (CUDA) and Host (C++) libraries to avoid compiler conflicts
set(CUDA_SOURCES CUDAMiner_cuda.cu)
set(CXX_SOURCES CUDAMiner.cpp)

set(HEADERS
	CUDAMiner.h
	CUDAMiner_cuda.h
	${CMAKE_CURRENT_BINARY_DIR}/CUDAMiner_kernel.h
)

# Find CUDA runtime libraries
find_package(CUDAToolkit REQUIRED)

# Fix for missing Boost headers in CUDAMiner.cpp
find_package(Boost REQUIRED)

# --- Device Library (CUDA) ---
add_library(ethash-cuda-device STATIC ${CUDA_SOURCES})
add_dependencies(ethash-cuda-device cuda_kernel)
target_include_directories(ethash-cuda-device PRIVATE .. ${CMAKE_CURRENT_BINARY_DIR})
target_link_libraries(ethash-cuda-device PRIVATE CUDA::cudart_static CUDA::cuda_driver CUDA::nvrtc)

set_target_properties(ethash-cuda-device PROPERTIES
	CUDA_SEPARABLE_COMPILATION OFF
	CUDA_RESOLVE_DEVICE_SYMBOLS OFF
	POSITION_INDEPENDENT_CODE ON
)

# Note: We do NOT disable parallel compilation here.
# The --threads 0 flag in target_compile_options enables parallel CUDA compilation.
# VS_GLOBAL_UseMultiToolTask would conflict with --threads 0, so we let NVCC handle it.

target_compile_options(ethash-cuda-device PRIVATE
	# CUDA compiler options:
	# --use_fast_math = Aggressive floating-point optimizations for mining performance
	# --expt-relaxed-constexpr = Allow constexpr in device code
	# --threads 0 = Parallel compilation using all CPU threads (reduces build time significantly)
	# -Xcompiler=/MD = Pass /MD to host C++ compiler (matches Rust runtime, set here for CUDA-specific build)
	# -Xcompiler=/W3 = Warning level 3
	# -Xcompiler=/wd4819 = Disable codepage warnings on Asian locale systems
	$<$<COMPILE_LANGUAGE:CUDA>: --use_fast_math --expt-relaxed-constexpr --threads 0 -Xcompiler=/MD -Xcompiler=/W3 -Xcompiler=/wd4819 >
)

# --- Host Library (C++) ---
add_library(ethash-cuda STATIC ${CXX_SOURCES} ${HEADERS})
target_include_directories(ethash-cuda PRIVATE .. ${CMAKE_CURRENT_BINARY_DIR} ${Boost_INCLUDE_DIRS})

target_link_libraries(ethash-cuda
	PRIVATE
		ethash-cuda-device
		devcore
		ethash
		progpow
		CUDA::cudart_static
		CUDA::nvrtc
		CUDA::cuda_driver
		${Boost_LIBRARIES}
)

if(MSVC)
	# Runtime library flag (/MD) is set in build.rs for consistency.
	# We only set warning level here.
	target_compile_options(ethash-cuda PRIVATE /W3)
endif()
