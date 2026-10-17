package org.kpc.rescaleframecraft.mixin;

import org.kpc.rescaleframecraft.render.NativeRenderer;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Pseudo;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

/** LTS 3.0.1: drivers have published state, before bindings/movement/gyro consume it. */
@Pseudo
@Mixin(targets="dev.isxander.controlify.Controlify",remap=false)
abstract class ControlifyInputMixin {
    @Inject(method="tickActiveController",at=@At("HEAD"),require=0)
    private void rfc$sample(CallbackInfo ci) {NativeRenderer.input(4);}
}
