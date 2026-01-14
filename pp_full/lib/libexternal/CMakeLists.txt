set(SOURCES progpow.h progpow_impl.cpp)

include_directories(BEFORE ..)

if (MSVC)
	add_library(ppow_progpow STATIC ${SOURCES})
else()
	add_library(ppow_progpow SHARED ${SOURCES})
endif()

target_link_libraries(ppow_progpow ethcore ethash progpow)

if(ETHASHCL)
	target_link_libraries(ppow_progpow ethash-cl)
endif()
if(ETHASHCUDA)
	target_link_libraries(ppow_progpow ethash-cuda)
endif()
