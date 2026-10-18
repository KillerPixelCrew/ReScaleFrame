package org.kpc.rescaleframecraft.mixin.flywheel;

import com.llamalad7.mixinextras.injector.ModifyExpressionValue;
import org.kpc.rescaleframecraft.render.TemporalRenderer;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Pseudo;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Slice;

@Pseudo
@Mixin(targets="com.seibel.distanthorizons.core.render.renderer.LodRenderer",remap=false)
abstract class DistantHorizonsTaaMixin {
    @ModifyExpressionValue(method="renderTerrain(Lcom/seibel/distanthorizons/core/render/RenderParams;Lcom/seibel/distanthorizons/core/wrapperInterfaces/minecraft/IProfilerWrapper;Z)V",
        at=@At(value="INVOKE",target="Lcom/seibel/distanthorizons/core/config/types/ConfigEntry;get()Ljava/lang/Object;"),
        slice=@Slice(from=@At(value="FIELD",target="Lcom/seibel/distanthorizons/core/config/Config$Client$Advanced$Graphics;enableAntiAliasing:Lcom/seibel/distanthorizons/core/config/types/ConfigEntry;"),
            to=@At(value="FIELD",target="Lcom/seibel/distanthorizons/core/config/Config$Client$Advanced$Debugging$DebugWireframe;enableRendering:Lcom/seibel/distanthorizons/core/config/types/ConfigEntry;")))
    private Object rfc$temporalOwner(Object enabled) {return TemporalRenderer.capturing()?Boolean.FALSE:enabled;}
}
