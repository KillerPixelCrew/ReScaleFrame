package org.kpc.rescaleframecraft.mixin.flywheel;

import org.kpc.rescaleframecraft.render.flywheel.FlywheelMotion;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Pseudo;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

@Pseudo
@Mixin(targets="dev.djefrey.colorwheel.indirect.ClrwlIndirectCullingGroup",remap=false)
abstract class ColorwheelGroupMixin {
    @Inject(method={"submitSolid","submitTranslucent","submitOitAsTranslucent","submitOit"},at=@At("HEAD"))
    private void rfc$history(CallbackInfo ci) {FlywheelMotion.colorwheelIndirect(this);}
    @Inject(method="bindForCrumbling",at=@At("HEAD"))
    private void rfc$crumbling(CallbackInfoReturnable<Boolean> cir) {FlywheelMotion.colorwheelIndirect(this);}
}
