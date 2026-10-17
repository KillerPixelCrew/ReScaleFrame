package org.kpc.rescaleframecraft.mixin.flywheel;

import org.kpc.rescaleframecraft.render.flywheel.FlywheelMotion;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Pseudo;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Pseudo
@Mixin(targets="dev.engine_room.flywheel.backend.engine.indirect.IndirectCullingGroup",remap=false)
abstract class FlywheelGroupMixin {
    @Inject(method={"submitSolid","submitTransparent","bindForCrumbling"},at=@At("HEAD"))
    private void rfc$history(CallbackInfo ci) {FlywheelMotion.indirect(this);}
}
