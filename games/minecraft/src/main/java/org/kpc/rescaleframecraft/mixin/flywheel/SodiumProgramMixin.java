package org.kpc.rescaleframecraft.mixin.flywheel;

import net.caffeinemc.mods.sodium.client.gl.shader.GlProgram;
import org.kpc.rescaleframecraft.render.flywheel.GpuMotion;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Mixin(value=GlProgram.class,remap=false)
abstract class SodiumProgramMixin {
    @Inject(method="delete",at=@At("HEAD"))
    private void rfc$retire(CallbackInfo ci) {GpuMotion.forget(((GlProgram<?>)(Object)this).handle());}
}
