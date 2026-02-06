cmake_minimum_required(VERSION 3.22.1)
project(wavry_android)

add_library(wavry_android SHARED native-lib.cpp)

set(WAVRY_FFI_PREBUILT "${CMAKE_CURRENT_SOURCE_DIR}/prebuilt/${ANDROID_ABI}/libwavry_ffi.a")
if(NOT EXISTS "${WAVRY_FFI_PREBUILT}")
    message(FATAL_ERROR "Missing ${WAVRY_FFI_PREBUILT}. Build Android Rust FFI with ./scripts/build-android-ffi.sh")
endif()

add_library(wavry_ffi STATIC IMPORTED)
set_target_properties(wavry_ffi PROPERTIES IMPORTED_LOCATION "${WAVRY_FFI_PREBUILT}")

target_include_directories(wavry_android PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}/include")

find_library(log-lib log)
find_library(android-lib android)
find_library(dl-lib dl)
find_library(m-lib m)

target_link_libraries(
    wavry_android
    wavry_ffi
    ${log-lib}
    ${android-lib}
    ${dl-lib}
    ${m-lib}
)
