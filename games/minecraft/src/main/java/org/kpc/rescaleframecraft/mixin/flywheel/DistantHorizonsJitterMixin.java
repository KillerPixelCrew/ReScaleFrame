package org.kpc.rescaleframecraft.mixin.flywheel;

import com.llamalad7.mixinextras.injector.ModifyExpressionValue;
import com.seibel.distanthorizons.api.methods.events.sharedParameterObjects.DhApiRenderParam;
import org.kpc.rescaleframecraft.render.DistantHorizonsIntegration;
import org.kpc.rescaleframecraft.render.TemporalRenderer;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Pseudo;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Slice;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Pseudo
@Mixin(targets="com.seibel.distanthorizons.common.render.openGl.terrain.GlDhTerrainShaderProgram",remap=false)
abstract class DistantHorizonsJitterMixin {
    @Inject(method="fillUniformData",at=@At("HEAD"))
    private void rfc$projection(DhApiRenderParam parameters,CallbackInfo ci) {DistantHorizonsIntegration.captureProjection(parameters.dhProjectionMatrix);}
    @ModifyExpressionValue(method="fillUniformData",at=@At(value="INVOKE",target="Lcom/seibel/distanthorizons/core/config/types/ConfigEntry;get()Ljava/lang/Object;"),
        slice=@Slice(from=@At(value="FIELD",target="Lcom/seibel/distanthorizons/core/config/Config$Client$Advanced$Graphics;enableAntiAliasing:Lcom/seibel/distanthorizons/core/config/types/ConfigEntry;"),
            to=@At(value="FIELD",target="Lcom/seibel/distanthorizons/core/config/Config$Client$Advanced$Graphics$NoiseTexture;enableNoiseTexture:Lcom/seibel/distanthorizons/core/config/types/ConfigEntry;")))
    private Object rfc$sceneJitter(Object enabled) {return TemporalRenderer.capturing()?Boolean.FALSE:enabled;}
}
