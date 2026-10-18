package org.kpc.rescaleframecraft.mixin.flywheel;

import com.llamalad7.mixinextras.injector.wrapmethod.WrapMethod;
import com.llamalad7.mixinextras.injector.wrapoperation.Operation;
import net.irisshaders.iris.pipeline.WorldRenderingPipeline;
import net.minecraft.client.Camera;
import net.minecraft.client.renderer.GameRenderer;
import org.joml.Matrix4f;
import org.joml.Matrix4fc;
import org.kpc.rescaleframecraft.render.TemporalRenderer;
import org.kpc.rescaleframecraft.render.flywheel.HandMotion;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Pseudo;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.ModifyArg;

@Pseudo
@Mixin(targets="net.irisshaders.iris.pathways.HandRenderer",remap=false)
abstract class IrisHandMixin {
    @WrapMethod(method="renderSolid")
    private void rfc$solid(Matrix4fc model,float tick,Camera camera,GameRenderer renderer,WorldRenderingPipeline pipeline,Operation<Void> original) {
        HandMotion.render(true,()->original.call(model,tick,camera,renderer,pipeline));
    }
    @WrapMethod(method="renderTranslucent")
    private void rfc$translucent(Matrix4fc model,float tick,Camera camera,GameRenderer renderer,WorldRenderingPipeline pipeline,Operation<Void> original) {
        HandMotion.render(false,()->original.call(model,tick,camera,renderer,pipeline));
    }
    @ModifyArg(method="setupGlState",at=@At(value="INVOKE",target="Lnet/minecraft/client/renderer/GameRenderer;resetProjectionMatrix(Lorg/joml/Matrix4f;)V",remap=true),index=0)
    private Matrix4f rfc$handJitter(Matrix4f projection) {
        if(!TemporalRenderer.capturing())return projection;
        var camera=TemporalRenderer.CAMERA;
        return new Matrix4f().translation(2*camera.jitter.x/camera.width(),2*camera.jitter.y/camera.height(),0).mul(projection);
    }
}
