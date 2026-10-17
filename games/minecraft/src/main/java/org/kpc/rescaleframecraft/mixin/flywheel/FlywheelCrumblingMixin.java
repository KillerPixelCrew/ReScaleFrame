package org.kpc.rescaleframecraft.mixin.flywheel;

import com.llamalad7.mixinextras.injector.wrapoperation.Operation;
import com.llamalad7.mixinextras.injector.wrapoperation.WrapOperation;
import org.kpc.rescaleframecraft.render.flywheel.FlywheelMotion;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Pseudo;
import org.spongepowered.asm.mixin.injection.At;

@Pseudo
@Mixin(targets={"dev.engine_room.flywheel.backend.engine.indirect.IndirectDrawManager",
    "dev.djefrey.colorwheel.indirect.ClrwlIndirectDrawManager"},remap=false)
abstract class FlywheelCrumblingMixin {
    @WrapOperation(method="renderCrumbling",at=@At(value="INVOKE",target="Lorg/lwjgl/opengl/GL40;glDrawElementsIndirect(IIJ)V"))
    private void rfc$motion(int mode,int type,long indirect,Operation<Void> original) {
        FlywheelMotion.draw(()->original.call(mode,type,indirect));
    }
}
