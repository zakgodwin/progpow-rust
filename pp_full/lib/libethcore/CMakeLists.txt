set(SOURCES
	# BlockHeader.h BlockHeader.cpp
	EthashAux.h EthashAux.cpp
	Exceptions.h
	work.h
)

include_directories(BEFORE ..)

find_package(Boost REQUIRED)

add_library(ethcore ${SOURCES})
target_include_directories(ethcore PUBLIC ${Boost_INCLUDE_DIRS})
if(Boost_LIBRARIES)
    target_link_libraries(ethcore PRIVATE ${Boost_LIBRARIES})
endif()

target_link_libraries(ethcore ethash devcore)

if(ETHASHCL)
	target_link_libraries(ethcore ethash-cl)
endif()
if(ETHASHCUDA)
	target_link_libraries(ethcore ethash-cuda)
endif()
