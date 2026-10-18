package org.kpc.rescaleframecraft.mixin.flywheel;

import org.kpc.rescaleframecraft.render.TemporalRenderer;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Pseudo;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Pseudo
@Mixin(targets="net.irisshaders.iris.pipeline.IrisRenderingPipeline",remap=false)
abstract class IrisPipelineMixin {
    @Inject(method="destroy",at=@At("HEAD"))
    private void rfc$pipelineRetired(CallbackInfo ci) {TemporalRenderer.reset();}
    @Inject(method="finalizeGameRendering",at=@At("TAIL"))
    private void rfc$worldComplete(CallbackInfo ci) {if(TemporalRenderer.irisFrame())TemporalRenderer.finish();}
}
