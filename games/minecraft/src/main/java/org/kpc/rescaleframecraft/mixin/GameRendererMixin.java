package org.kpc.rescaleframecraft.mixin;

import net.minecraft.client.renderer.GameRenderer;
import org.kpc.rescaleframecraft.render.TemporalRenderer;
import org.kpc.rescaleframecraft.render.NativeRenderer;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Mixin(GameRenderer.class)
public abstract class GameRendererMixin {
    @Inject(method="renderLevel",at=@At("HEAD"))
    private void rfc$worldTarget(CallbackInfo ci) {NativeRenderer.beginWorld();}
    @Inject(method="renderLevel",at=@At(value="INVOKE",target="Lcom/mojang/blaze3d/systems/RenderSystem;clear(IZ)V"))
    private void rfc$beforeHand(CallbackInfo ci) { TemporalRenderer.finish(); }
    @Inject(method="renderLevel",at=@At("RETURN"))
    private void rfc$withoutHand(CallbackInfo ci) { TemporalRenderer.finish(); }
}
