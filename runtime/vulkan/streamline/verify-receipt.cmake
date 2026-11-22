# SPDX-License-Identifier: MIT
# Verifies a corrected Streamline Vulkan runtime against its streamline-sdk-build.json receipt and the
# tracked source corrections, and yields the interposer's SHA-256 (lowercase hex). The Runtime build
# includes this file; the build scripts run it as
#   cmake -DRUNTIME=<runtime directory> -P runtime/vulkan/streamline/verify-receipt.cmake
# which prints the digest on stdout.
set(RSF_STREAMLINE_SOURCE_REVISION "2122257e0fce486f91b385aa63b9a09b0a34b363")
set(RSF_STREAMLINE_RECEIPT_DIR "${CMAKE_CURRENT_LIST_DIR}")

function(rsf_streamline_receipt runtime out)
    file(READ "${runtime}/streamline-sdk-build.json" receipt)
    string(JSON revision GET "${receipt}" sdkCommit)
    if(NOT revision STREQUAL RSF_STREAMLINE_SOURCE_REVISION)
        message(FATAL_ERROR "Unexpected corrected Streamline source revision ${revision}")
    endif()
    foreach(input "patchSha256|sdk-fixes/prepare.py" "dispatchSha256|sdk-fixes/ngx-vulkan-dispatch.h"
            "recipeSha256|build-sdk.ps1" "presentCaptureSha256|present-capture.h"
            "selectedDevicePatchSha256|sdk-fixes/selected_device.py"
            "selectedDeviceConfigSha256|sdk-fixes/selected-device-config.h")
        string(REPLACE "|" ";" fields "${input}")
        list(GET fields 0 field)
        list(GET fields 1 source)
        file(SHA256 "${RSF_STREAMLINE_RECEIPT_DIR}/${source}" actual)
        string(JSON expected GET "${receipt}" "${field}")
        string(TOLOWER "${expected}" expected)
        if(NOT actual STREQUAL expected)
            message(FATAL_ERROR "Corrected Streamline receipt differs from ${source}")
        endif()
    endforeach()
    foreach(binary sl.interposer.dll sl.common.dll)
        file(SHA256 "${runtime}/${binary}" actual)
        string(JSON expected GET "${receipt}" binaries "${binary}")
        string(TOLOWER "${expected}" expected)
        if(NOT actual STREQUAL expected)
            message(FATAL_ERROR "Corrected Streamline binary differs from receipt: ${binary}")
        endif()
    endforeach()
    file(SHA256 "${runtime}/sl.interposer.dll" digest)
    set(${out} "${digest}" PARENT_SCOPE)
endfunction()

if(CMAKE_SCRIPT_MODE_FILE STREQUAL CMAKE_CURRENT_LIST_FILE)
    rsf_streamline_receipt("${RUNTIME}" digest)
    execute_process(COMMAND "${CMAKE_COMMAND}" -E echo "${digest}")
endif()
