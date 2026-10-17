package org.kpc.rescaleframecraft.mixin.flywheel;

import com.llamalad7.mixinextras.injector.wrapoperation.Operation;
import com.llamalad7.mixinextras.injector.wrapoperation.WrapOperation;
import org.kpc.rescaleframecraft.render.flywheel.FlywheelMotion;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Pseudo;
import org.spongepowered.asm.mixin.injection.At;

@Pseudo
@Mixin(targets="dev.engine_room.flywheel.backend.gl.GlCompat",remap=false)
abstract class FlywheelIndirectDrawMixin {
    @WrapOperation(method="safeMultiDrawElementsIndirect",at=@At(value="INVOKE",target="Lorg/lwjgl/opengl/GL43;glMultiDrawElementsIndirect(IIJII)V"))
    private static void rfc$multi(int mode,int type,long pointer,int count,int stride,Operation<Void> original) {
        FlywheelMotion.draw(()->original.call(mode,type,pointer,count,stride));
    }
    @WrapOperation(method="safeMultiDrawElementsIndirect",at=@At(value="INVOKE",target="Lorg/lwjgl/opengl/GL40;glDrawElementsIndirect(IIJ)V"))
    private static void rfc$single(int mode,int type,long pointer,Operation<Void> original) {
        FlywheelMotion.draw(()->original.call(mode,type,pointer));
    }
}
