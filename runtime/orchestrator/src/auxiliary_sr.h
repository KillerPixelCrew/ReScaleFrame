// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/d3d11_host.h>
#include <d3d11.h>
#include <memory>

class AuxiliarySurfaces {
    struct State;
    std::unique_ptr<State> state_;
    bool render(const rsf_d3d11_auxiliary_pass*, bool capture);
public:
    AuxiliarySurfaces();
    ~AuxiliarySurfaces();
    bool prepare(ID3D11DeviceContext*, const rsf_game_render_pass&, const rsf_d3d11_auxiliary_pass&,
        rsf_game_log_fn, void*);
    // reserve performs only validation/allocation; seal snapshots/draws private products.
    bool reserve(ID3D11DeviceContext*, const rsf_game_render_pass&, const rsf_d3d11_auxiliary_pass&,
        rsf_game_log_fn, void*, bool recovery = false);
    bool refine(const rsf_d3d11_auxiliary_pass&);
    bool seal();
    bool cancel(); // Unsealed reservation only.
    bool sealed() const;
    ID3D11Texture2D* original_colour() const;
    bool temporal_prepare(const rsf_d3d11_auxiliary_pass&);
    bool spatial_prepare();
    bool commit(bool temporal = false);
    bool restore();
    bool matches(const rsf_d3d11_auxiliary_scope&) const;
    bool pending() const;
};
