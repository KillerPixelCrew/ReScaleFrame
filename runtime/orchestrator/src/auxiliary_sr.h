// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/d3d11_host.h>
#include <d3d11.h>
#include <memory>

class AuxiliarySurfaces {
    struct State;
    std::unique_ptr<State> state_;
public:
    AuxiliarySurfaces();
    ~AuxiliarySurfaces();
    bool prepare(ID3D11DeviceContext*, const rsf_game_render_pass&, const rsf_d3d11_auxiliary_pass&,
        rsf_game_log_fn, void*);
    bool commit();
    bool restore();
    bool matches(const rsf_d3d11_auxiliary_scope&) const;
    bool pending() const;
};
