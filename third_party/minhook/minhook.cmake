# Provisions the pinned MinHook files for the Cargo build (cmake -P). CMake itself fetches the
# same revision with FetchContent; both read it from minhook.lock.json.
if(NOT DEFINED RSF_MINHOOK)
    message(FATAL_ERROR "RSF_MINHOOK names the directory to provision")
endif()
file(READ "${CMAKE_CURRENT_LIST_DIR}/minhook.lock.json" rsf_minhook_lock)
string(JSON rsf_minhook_rev GET "${rsf_minhook_lock}" revision)
string(JSON rsf_minhook_count LENGTH "${rsf_minhook_lock}" files)
math(EXPR rsf_minhook_last "${rsf_minhook_count} - 1")
foreach(rsf_minhook_index RANGE ${rsf_minhook_last})
    string(JSON rsf_minhook_file MEMBER "${rsf_minhook_lock}" files ${rsf_minhook_index})
    string(JSON rsf_minhook_hash GET "${rsf_minhook_lock}" files "${rsf_minhook_file}")
    set(rsf_minhook_dest "${RSF_MINHOOK}/${rsf_minhook_file}")
    if(NOT EXISTS "${rsf_minhook_dest}")
        # Download beside the destination and rename only a verified file, so an interrupted
        # download cannot leave a partial copy that every later build refuses.
        file(DOWNLOAD "https://raw.githubusercontent.com/TsudaKageyu/minhook/${rsf_minhook_rev}/${rsf_minhook_file}"
            "${rsf_minhook_dest}.download" EXPECTED_HASH "SHA256=${rsf_minhook_hash}" TLS_VERIFY ON)
        file(RENAME "${rsf_minhook_dest}.download" "${rsf_minhook_dest}")
    endif()
    # The lock pins the upstream LF bytes; a copy checked out with CRLF holds the same file.
    file(READ "${rsf_minhook_dest}" rsf_minhook_text)
    string(REPLACE "\r\n" "\n" rsf_minhook_text "${rsf_minhook_text}")
    string(SHA256 rsf_minhook_actual "${rsf_minhook_text}")
    if(NOT rsf_minhook_actual STREQUAL rsf_minhook_hash)
        message(FATAL_ERROR "Pinned MinHook file differs: ${rsf_minhook_file}")
    endif()
endforeach()