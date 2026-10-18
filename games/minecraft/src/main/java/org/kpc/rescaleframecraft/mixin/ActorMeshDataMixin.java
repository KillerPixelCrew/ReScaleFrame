package org.kpc.rescaleframecraft.mixin;

import com.mojang.blaze3d.vertex.MeshData;
import org.kpc.rescaleframecraft.render.flywheel.ActorMotion;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Mixin(MeshData.class)
abstract class ActorMeshDataMixin {
    @Inject(method="close",at=@At("HEAD"))
    private void rfc$closed(CallbackInfo ci) {ActorMotion.discard((MeshData)(Object)this);}
}
