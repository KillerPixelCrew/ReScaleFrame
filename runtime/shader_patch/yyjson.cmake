# yyjson is the sole incorporated shader-patch dependency. Store it under vendor/.
if(NOT DEFINED RSF_PATCH_YYJSON)
    set(RSF_PATCH_YYJSON "${PROJECT_SOURCE_DIR}/vendor/shader-patch/yyjson")
endif()
file(READ "${CMAKE_CURRENT_LIST_DIR}/yyjson.lock.json" rsf_json_lock)
string(JSON rsf_json_rev GET "${rsf_json_lock}" revision)
foreach(rsf_json_file "src/yyjson.c" "src/yyjson.h" "LICENSE")
    string(JSON rsf_json_hash GET "${rsf_json_lock}" files "${rsf_json_file}")
    set(rsf_json_dest "${RSF_PATCH_YYJSON}/${rsf_json_file}")
    if(NOT EXISTS "${rsf_json_dest}")
        get_filename_component(rsf_json_parent "${rsf_json_dest}" DIRECTORY)
        file(MAKE_DIRECTORY "${rsf_json_parent}")
        file(DOWNLOAD "https://raw.githubusercontent.com/ibireme/yyjson/${rsf_json_rev}/${rsf_json_file}"
            "${rsf_json_dest}" EXPECTED_HASH "SHA256=${rsf_json_hash}" TLS_VERIFY ON)
    endif()
    # The lock pins the upstream LF bytes. A copy checked out with CRLF line
    # endings holds the same file, so hash it with LF endings.
    file(READ "${rsf_json_dest}" rsf_json_text)
    string(REPLACE "\r\n" "\n" rsf_json_text "${rsf_json_text}")
    string(SHA256 rsf_json_actual "${rsf_json_text}")
    if(NOT rsf_json_actual STREQUAL rsf_json_hash)
        message(FATAL_ERROR "Pinned yyjson file differs: ${rsf_json_file}")
    endif()
endforeach()
