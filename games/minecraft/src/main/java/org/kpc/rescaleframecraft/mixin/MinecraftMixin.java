package org.kpc.rescaleframecraft.mixin;

import com.mojang.blaze3d.pipeline.RenderTarget;
import net.minecraft.client.Minecraft;
import org.kpc.rescaleframecraft.render.NativeRenderer;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

@Mixin(Minecraft.class)
public abstract class MinecraftMixin {
    @Inject(method="runTick",at=@At("HEAD"))
    private void rfc$input(boolean tick,CallbackInfo ci) {NativeRenderer.beginFrame();}
    @Inject(method="runTick",at=@At(value="INVOKE",target="Lnet/neoforged/neoforge/client/ClientHooks;fireRenderFramePre(Lnet/minecraft/client/DeltaTracker;)V",shift=At.Shift.AFTER))
    private void rfc$simulationEnd(boolean tick,CallbackInfo ci) {NativeRenderer.simulationEnd();}
    @Inject(method="getMainRenderTarget",at=@At("HEAD"),cancellable=true)
    private void rfc$worldTarget(CallbackInfoReturnable<RenderTarget> cir) {
        var target=NativeRenderer.worldTarget();if(target!=null)cir.setReturnValue(target);
    }
    @Inject(method="close",at=@At("HEAD"))
    private void rfc$close(CallbackInfo ci) {NativeRenderer.shutdown();}
}
