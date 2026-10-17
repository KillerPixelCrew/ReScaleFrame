package org.kpc.rescaleframecraft.mixin.flywheel;

import com.llamalad7.mixinextras.injector.wrapoperation.Operation;
import com.llamalad7.mixinextras.injector.wrapoperation.WrapOperation;
import org.kpc.rescaleframecraft.render.flywheel.FlywheelMotion;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Pseudo;
import org.spongepowered.asm.mixin.injection.At;

@Pseudo
@Mixin(targets={"dev.engine_room.flywheel.backend.engine.MeshPool$PooledMesh","dev.djefrey.colorwheel.engine.ClrwlMeshPool$PooledMesh"},remap=false)
abstract class FlywheelMeshDrawMixin {
    @WrapOperation(method="draw",at=@At(value="INVOKE",target="Lorg/lwjgl/opengl/GL32;glDrawElementsInstancedBaseVertex(IIIJII)V"))
    private void rfc$instanced(int mode,int count,int type,long indices,int instances,int base,Operation<Void> original) {
        FlywheelMotion.draw(()->original.call(mode,count,type,indices,instances,base));
    }
    @WrapOperation(method="draw",at=@At(value="INVOKE",target="Lorg/lwjgl/opengl/GL32;glDrawElementsBaseVertex(IIIJI)V"))
    private void rfc$single(int mode,int count,int type,long indices,int base,Operation<Void> original) {
        FlywheelMotion.draw(()->original.call(mode,count,type,indices,base));
    }
}
