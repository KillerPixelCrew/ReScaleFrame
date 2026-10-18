package org.kpc.rescaleframecraft.mixin;

import net.minecraft.client.renderer.ShaderInstance;
import org.kpc.rescaleframecraft.render.flywheel.GpuMotion;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Mixin(ShaderInstance.class)
public abstract class ShaderInstanceMixin {
    @Inject(method="close",at=@At("HEAD"))
    private void rfc$retireMotionProgram(CallbackInfo ci) {GpuMotion.forget(((ShaderInstance)(Object)this).getId());}
}
