package org.kpc.rescaleframecraft.mixin;

import org.kpc.rescaleframecraft.render.NativeRenderer;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Pseudo;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

/** 0.25.4: only reached after the active controller passes accessibility checks. */
@Pseudo
@Mixin(targets="com.mrcrayfish.controllable.client.InputProcessor",remap=false)
abstract class ControllableInputMixin {
    @Inject(method="gatherAndQueueControllerInput",at=@At(value="INVOKE",
        target="Lcom/mrcrayfish/controllable/client/input/Controller;captureButtonStates()Lcom/mrcrayfish/controllable/client/input/ButtonStates;"),require=0)
    private void rfc$sample(CallbackInfo ci) {NativeRenderer.input(4);}
}
