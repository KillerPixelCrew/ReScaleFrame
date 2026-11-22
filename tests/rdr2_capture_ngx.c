/* SPDX-License-Identifier: MIT */
/* The fixture's NGX parameter object is a native C++ object whose Set overloads are called through
   its virtual table. Each call is contained at the foreign boundary, as the ASI's read leaf is. */
#include <stdbool.h>
#include <stdint.h>
#include <windows.h>
#include <rescaleframe/foreign_guard.h>

/* MSVC reverses the overloads within each named group of tests/vendor_census_ngx_params.h, which
   keeps the SDK declaration order. tests/loader/src/windows/vendor_census_fixture/ngx_params.rs
   builds the object with the same table, and vendor_census_native.cpp calls it through the C++
   declaration. */
enum {
    SET_POINTER = 0,
    SET_D3D12 = 1,
    SET_INT = 3,
    SET_UINT = 4,
    SET_FLOAT = 6,
};

typedef void (*set_pointer_fn)(void* parameters, const char* name, void* value);
typedef void (*set_int_fn)(void* parameters, const char* name, int value);
typedef void (*set_uint_fn)(void* parameters, const char* name, unsigned int value);
typedef void (*set_float_fn)(void* parameters, const char* name, float value);

static void** slots(void* parameters) { return *(void***)parameters; }

static void set_flags(void* parameters)
{
    ((set_int_fn)slots(parameters)[SET_INT])(parameters, "DLSS.Feature.Create.Flags", 1 | 2 | 8);
}

static void set_inputs(void* parameters, void* color, void* depth, void* motion, void* output)
{
    void** table = slots(parameters);
    set_pointer_fn resource = (set_pointer_fn)table[SET_D3D12];
    set_float_fn real = (set_float_fn)table[SET_FLOAT];
    set_uint_fn unsigned_value = (set_uint_fn)table[SET_UINT];
    resource(parameters, "Color", color);
    resource(parameters, "Depth", depth);
    resource(parameters, "MotionVectors", motion);
    resource(parameters, "Output", output);
    real(parameters, "Jitter.Offset.X", 0.25f);
    real(parameters, "Jitter.Offset.Y", -0.3888889f);
    real(parameters, "MV.Scale.X", 1.f);
    real(parameters, "MV.Scale.Y", 1.f);
    unsigned_value(parameters, "DLSS.Render.Subrect.Dimensions.Width", 32u);
    unsigned_value(parameters, "DLSS.Render.Subrect.Dimensions.Height", 24u);
    ((set_int_fn)table[SET_INT])(parameters, "Reset", 0);
}

bool rsf_rdr2_fixture_ngx_flags(void* parameters)
{
    bool called = true;
    RSF_FOREIGN_TRY { set_flags(parameters); }
    RSF_FAULT_CATCH { called = false; }
    return called;
}

bool rsf_rdr2_fixture_ngx_inputs(void* parameters, void* color, void* depth, void* motion, void* output)
{
    bool called = true;
    RSF_FOREIGN_TRY { set_inputs(parameters, color, depth, motion, output); }
    RSF_FAULT_CATCH { called = false; }
    return called;
}