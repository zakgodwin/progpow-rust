# Modern CMake CUDA library configuration for CUDA 13.1+ and Visual Studio 2022
cmake_minimum_required(VERSION 3.18)

# Force Release mode for CUDA compilation (avoid debug runtime issues)
if(NOT CMAKE_BUILD_TYPE)
	set(CMAKE_BUILD_TYPE Release CACHE STRING "Build type" FORCE)
endif()

# Enable CUDA language support
enable_language(CUDA)

# Set CUDA standard
set(CMAKE_CUDA_STANDARD 14)
set(CMAKE_CUDA_STANDARD_REQUIRED ON)

# CUDA architectures for CUDA 13.1 (sm_75 Turing through sm_120 Blackwell)
set(CMAKE_CUDA_ARCHITECTURES "75;80;86;89;90;120")

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

if(MSVC)
	string(REPLACE "/MP" "" CMAKE_CUDA_FLAGS "${CMAKE_CUDA_FLAGS}")
	set_target_properties(ethash-cuda-device PROPERTIES VS_GLOBAL_UseMultiToolTask "false")
endif()

target_compile_options(ethash-cuda-device PRIVATE
	$<$<COMPILE_LANGUAGE:CUDA>: --use_fast_math --expt-relaxed-constexpr -Xcompiler=/MD -Xcompiler=/W3 -Xcompiler=/wd4819 >
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
	target_compile_options(ethash-cuda PRIVATE /MD /W3)
endif()
