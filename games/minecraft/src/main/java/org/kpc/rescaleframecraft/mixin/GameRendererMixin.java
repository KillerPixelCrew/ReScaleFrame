package org.kpc.rescaleframecraft.mixin;

import net.minecraft.client.renderer.GameRenderer;
import com.llamalad7.mixinextras.injector.wrapoperation.Operation;
import com.llamalad7.mixinextras.injector.wrapoperation.WrapOperation;
import org.kpc.rescaleframecraft.render.TemporalRenderer;
import org.kpc.rescaleframecraft.render.NativeRenderer;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Mixin(value=GameRenderer.class,priority=500)
public abstract class GameRendererMixin {
    @Inject(method="renderLevel",at=@At("HEAD"))
    private void rfc$worldTarget(CallbackInfo ci) {NativeRenderer.beginWorld();}
    @Inject(method="renderLevel",at=@At(value="INVOKE",target="Lcom/mojang/blaze3d/systems/RenderSystem;clear(IZ)V"))
    private void rfc$beforeHand(CallbackInfo ci) {if(!TemporalRenderer.irisFrame())TemporalRenderer.finish();}
    @WrapOperation(method="renderLevel",at=@At(value="INVOKE",target="Lcom/mojang/blaze3d/systems/RenderSystem;clear(IZ)V"))
    private void rfc$preserveIrisDepth(int mask,boolean checkError,Operation<Void> original) {
        // Iris already rendered both hand passes in the world pipeline. Its cancelled
        // vanilla hand pass must not erase the depth needed by the late temporal resolve.
        if(TemporalRenderer.irisFrame())mask&=~org.lwjgl.opengl.GL11.GL_DEPTH_BUFFER_BIT;
        if(mask!=0)original.call(mask,checkError);
    }
    @Inject(method="renderLevel",at=@At("RETURN"))
    private void rfc$withoutHand(CallbackInfo ci) {TemporalRenderer.finish();}
}
