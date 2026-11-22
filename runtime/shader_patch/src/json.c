// SPDX-License-Identifier: MIT
// Keep yyjson's inline accessors behind the visitor ABI used by the Rust pack builder.
#include "json.h"
#include <yyjson.h>
#include <stdlib.h>

typedef struct {
    yyjson_val* next;
    size_t left;
    bool object;
} level;

int32_t rsf_patch_parse_json(rsf_patch_bytes text, void* user, rsf_patch_json_visit visit)
{
    yyjson_doc* doc = yyjson_read((const char*)text.data, text.size, 0);
    if (!doc) return -1;

    // The Rust visitor refuses nodes at depth 128. Emit that node so it can report
    // the refusal, then stop without descending any further.
    level stack[128];
    size_t depth = 0;
    yyjson_val* value = yyjson_doc_get_root(doc);
    for (;;) {
        uint32_t kind = RSF_PATCH_JSON_OTHER;
        uint64_t number = 0;
        const char* str = NULL;
        size_t size = 0;
        if (yyjson_is_str(value)) {
            kind = RSF_PATCH_JSON_STRING;
            str = yyjson_get_str(value);
            size = yyjson_get_len(value);
        } else if (yyjson_is_int(value)) {
            kind = yyjson_is_uint(value) ? RSF_PATCH_JSON_UINT : RSF_PATCH_JSON_SINT;
            number = yyjson_get_uint(value);
        } else if (yyjson_is_arr(value)) {
            kind = RSF_PATCH_JSON_ARRAY;
            size = yyjson_arr_size(value);
        } else if (yyjson_is_obj(value)) {
            kind = RSF_PATCH_JSON_OBJECT;
            size = yyjson_obj_size(value);
        }
        visit(user, kind, number, str, size);
        if (depth == sizeof(stack) / sizeof(stack[0])) break;
        if (size && (kind == RSF_PATCH_JSON_ARRAY || kind == RSF_PATCH_JSON_OBJECT)) {
            stack[depth++] = (level){unsafe_yyjson_get_first(value), size, kind == RSF_PATCH_JSON_OBJECT};
        }
        while (depth && !stack[depth - 1].left) --depth;
        if (!depth) break;
        level* parent = &stack[depth - 1];
        --parent->left;
        value = parent->next;
        if (parent->object) {
            visit(user, RSF_PATCH_JSON_STRING, 0, yyjson_get_str(value), yyjson_get_len(value));
            ++value;
        }
        parent->next = unsafe_yyjson_get_next(value);
    }
    yyjson_doc_free(doc);
    return 0;
}
