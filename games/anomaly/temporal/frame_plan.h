#pragma once

#include "history.h"

namespace rsf::anomaly::temporal {

struct GeometryPassPlan {
    ProjectionDomain domain{};
    Extent viewport{};
    Mat4 view{};
    Mat4 projection{}; // Jittered raster projection, not the unjittered motion projection.
};

// Pixel-coordinate canvases deliberately have no scene projection/jitter field.
struct CanvasPassPlan {
    Extent viewport{};
};

// Main can also suppress Present (camera-ready path). This is policy, not a Present lease.
enum class PresentationRoute { RequiresHostDecision, OffscreenScope };
struct FrameViewPlan {
    FrameToken frame;
    GeometryPassPlan world;
    std::optional<GeometryPassPlan> first_person;
    Extent reconstructed_world;
    CanvasPassPlan screen_ui;
    CanvasPassPlan pda_content; // UV-space source texture, NOT output-space FG UI.
    PresentationRoute presentation{PresentationRoute::RequiresHostDecision};
};

inline std::optional<FrameViewPlan> plan_frame_view(const TemporalHistory& history, FrameToken frame)
{
    const auto input = history.current_frame(frame);
    if (!input) {
        return std::nullopt;
    }
    const auto world_projection = jitter_projection(input->world.projection,
        input->world.jitter_pixels, input->resolutions.render);
    if (!world_projection) {
        return std::nullopt;
    }
    std::optional<GeometryPassPlan> first_person;
    if (input->first_person) {
        const auto projection = jitter_projection(input->first_person->projection,
            input->first_person->jitter_pixels, input->resolutions.render);
        if (!projection) {
            return std::nullopt;
        }
        first_person = GeometryPassPlan{ProjectionDomain::FirstPerson, input->resolutions.render,
            input->first_person->view, *projection};
    }
    return FrameViewPlan{frame,
        {ProjectionDomain::World, input->resolutions.render, input->world.view, *world_projection},
        first_person, input->resolutions.output, {input->resolutions.output}, {input->resolutions.pda},
        input->view == ViewId::Scope ? PresentationRoute::OffscreenScope : PresentationRoute::RequiresHostDecision};
}

} // namespace rsf::anomaly::temporal
