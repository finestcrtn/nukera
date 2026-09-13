cmake_minimum_required(VERSION 3.22.1)

project(nukera_native)

file(GLOB BYE_DPI_SRC byedpi/*.c)
list(REMOVE_ITEM BYE_DPI_SRC ${CMAKE_CURRENT_SOURCE_DIR}/byedpi/win_service.c)

add_library(nukera SHARED ${BYE_DPI_SRC} native-lib.c)
target_include_directories(nukera PRIVATE byedpi)

target_compile_options(nukera PRIVATE -std=c99 -O2 -Wall -Wno-unused -Wextra -Wno-unused-parameter -pedantic)
target_compile_definitions(nukera PRIVATE ANDROID_APP)

target_link_libraries(nukera PRIVATE android log)
set(CMAKE_SHARED_LINKER_FLAGS "${CMAKE_SHARED_LINKER_FLAGS} -Wl,-z,max-page-size=16384")
