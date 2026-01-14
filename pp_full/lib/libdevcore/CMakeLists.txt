file(GLOB HEADERS "*.h")
file(GLOB SOURCES "*.cpp")

find_package(Threads)
find_package(Boost REQUIRED)
# include_directories(${Boost_INCLUDE_DIRS})

add_library(devcore ${SOURCES} ${HEADERS})
target_include_directories(devcore PUBLIC ${Boost_INCLUDE_DIRS})
if(Boost_LIBRARIES)
    target_link_libraries(devcore PUBLIC ${Boost_LIBRARIES})
endif()
target_link_libraries(devcore PRIVATE Threads::Threads)
