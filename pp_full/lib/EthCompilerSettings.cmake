# Set necessary compile and link flags

include(EthCheckCXXFlags.cmake)

# C++11 check and activation
if ("${CMAKE_CXX_COMPILER_ID}" MATCHES "GNU")

	set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS} -Wall -Wno-unknown-pragmas -Wextra -Wno-error=parentheses -pedantic")

    eth_add_cxx_compiler_flag_if_supported(-ffunction-sections)
    eth_add_cxx_compiler_flag_if_supported(-fdata-sections)
    eth_add_cxx_linker_flag_if_supported(-Wl,--gc-sections)

elseif ("${CMAKE_CXX_COMPILER_ID}" MATCHES "Clang")

	set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS} -Wall -Wno-unknown-pragmas -Wextra")

    eth_add_cxx_compiler_flag_if_supported(-ffunction-sections)
    eth_add_cxx_compiler_flag_if_supported(-fdata-sections)
    eth_add_cxx_linker_flag_if_supported(-Wl,--gc-sections)

	if ("${CMAKE_SYSTEM_NAME}" MATCHES "Linux")
		set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS} -stdlib=libstdc++ -fcolor-diagnostics -Qunused-arguments")
	endif()

elseif ("${CMAKE_CXX_COMPILER_ID}" STREQUAL "MSVC")

	# declare Windows XP requirement
	# undefine windows.h MAX & MIN macros because they conflict with std::min & std::max functions
	# disable unsafe CRT Library functions warnings
	add_definitions(/D_WIN32_WINNT=0x0501 /DNOMINMAX /D_CRT_SECURE_NO_WARNINGS)

	# MSVC Compiler flags for Rust FFI compatibility:
	# /MP  = Multi-processor compilation (parallel builds)
	# /EHsc = C++ exception handling
	# /GL  = Link Time Code Generation - DISABLED because it breaks Rust FFI
	# /wd4267 = Disable size_t conversion warnings
	# /wd4290 = Disable C++ exception specification warnings
	# /wd4503 = Disable decorated name length warnings
	#
	# IMPORTANT: Runtime library flags (/MD) are set in build.rs, NOT here.
	# This avoids CMake generator expression timing issues and keeps runtime
	# configuration in one canonical location.
	add_compile_options(
		$<$<COMPILE_LANGUAGE:CXX>:/MP>
		$<$<COMPILE_LANGUAGE:CXX>:/EHsc>
		$<$<COMPILE_LANGUAGE:CXX>:/wd4267>
		$<$<COMPILE_LANGUAGE:CXX>:/wd4290>
		$<$<COMPILE_LANGUAGE:CXX>:/wd4503>
	)

	# Linker flags: Enable optimizations in release, debug info in debug
	set(CMAKE_EXE_LINKER_FLAGS_RELEASE "${CMAKE_EXE_LINKER_FLAGS_RELEASE} /OPT:REF /OPT:ICF /RELEASE")
	set(CMAKE_EXE_LINKER_FLAGS_DEBUG "${CMAKE_EXE_LINKER_FLAGS_DEBUG} /DEBUG")
else ()
	message(WARNING "Your compiler is not tested, if you run into any issues, we'd welcome any patches.")
endif ()

set(SANITIZE NO CACHE STRING "Instrument build with provided sanitizer")
if(SANITIZE)
	set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS} -fno-omit-frame-pointer -fsanitize=${SANITIZE}")
endif()
