# A custom command and target to turn the CUDA kernel into a byte array header
# The normal build depends on it properly and if the kernel file is changed, then
# a rebuild of libethash-cuda should be triggered

# Only set specific compiler on Linux (Windows uses MSVC automatically)
if(UNIX AND NOT APPLE)
	set(CMAKE_C_COMPILER /usr/bin/gcc-7)
	set(CMAKE_CXX_COMPILER /usr/bin/g++-7)
endif()
# Target Blackwell (100), Hopper (90), and Ada (89) - RTX 50/40 series + data center
set(CUDA_ARCH 89;90;100)

add_custom_command(
	OUTPUT ${CMAKE_CURRENT_BINARY_DIR}/CUDAMiner_kernel.h
	COMMAND ${CMAKE_COMMAND} ARGS
	-DBIN2H_SOURCE_FILE="${CMAKE_CURRENT_SOURCE_DIR}/CUDAMiner_kernel.cu"
	-DBIN2H_VARIABLE_NAME=CUDAMiner_kernel
	-DBIN2H_HEADER_FILE="${CMAKE_CURRENT_BINARY_DIR}/CUDAMiner_kernel.h"
	-P "${CMAKE_CURRENT_SOURCE_DIR}/../bin2h.cmake"
	COMMENT "Generating CUDA Kernel Byte Array"
	DEPENDS ${CMAKE_CURRENT_SOURCE_DIR}/CUDAMiner_kernel.cu
)
add_custom_target(cuda_kernel DEPENDS ${CMAKE_CURRENT_BINARY_DIR}/CUDAMiner_kernel.h ${CMAKE_CURRENT_SOURCE_DIR}/CUDAMiner_kernel.cu)

find_package(CUDA REQUIRED)

set(CUDA_NVCC_FLAGS ${CUDA_NVCC_FLAGS};--ptxas-options=-v;-lineinfo)

if (MSVC)
	# Fix MSVC/CUDA compatibility: Disable extended attributes and use older C++ standard
	list(APPEND CUDA_NVCC_FLAGS "-Xcompiler /wd4819,/wd4244,/wd4267,/Zc:__cplusplus")
	list(APPEND CUDA_NVCC_FLAGS "--std=c++14")
	# Tell CUDA to ignore newer Windows SDK features
	add_definitions(-D_ENABLE_EXTENDED_ALIGNED_STORAGE)
endif()

if (NOT MSVC)
	list(APPEND CUDA_NVCC_FLAGS "--disable-warnings")
endif()

list(APPEND CUDA_NVCC_FLAGS_RELEASE -O3)
list(APPEND CUDA_NVCC_FLAGS_DEBUG -G)

if(COMPUTE AND (COMPUTE GREATER 0))
	list(APPEND CUDA_NVCC_FLAGS "-gencode arch=compute_${COMPUTE},code=sm_${COMPUTE}")
else()
	# Target Blackwell (100), Hopper (90), and Ada (89) architectures
	set(
		CUDA_NVCC_FLAGS
		${CUDA_NVCC_FLAGS}
		"-gencode arch=compute_89,code=sm_89"
		"-gencode arch=compute_90,code=sm_90"
		"-gencode arch=compute_100,code=sm_100"
	)
endif()

file(GLOB sources CUDAMiner.cpp CUDAMiner_cuda.cu)
file(GLOB headers CUDAMiner.h CUDAMiner_cuda.h ${CMAKE_CURRENT_BINARY_DIR}/CUDAMiner_kernel.h)

# Cmake doesn't handle nvrtc automatically
find_library(CUDA_nvrtc_LIBRARY nvrtc "${CUDA_TOOLKIT_ROOT_DIR}/lib64" "${CUDA_TOOLKIT_ROOT_DIR}/lib/x64")
find_library(CUDA_cuda_LIBRARY cuda "${CUDA_TOOLKIT_ROOT_DIR}/lib64" "${CUDA_TOOLKIT_ROOT_DIR}/lib/x64")

cuda_add_library(ethash-cuda STATIC ${sources} ${headers})
add_dependencies(ethash-cuda cuda_kernel)
target_link_libraries(ethash-cuda ethcore ethash progpow)
target_link_libraries(ethash-cuda ${CUDA_nvrtc_LIBRARY} ${CUDA_cuda_LIBRARY})
# Include our minimal boost stubs for Windows builds
target_include_directories(ethash-cuda PRIVATE .. ${CMAKE_CURRENT_BINARY_DIR} ${CMAKE_CURRENT_SOURCE_DIR}/../)
