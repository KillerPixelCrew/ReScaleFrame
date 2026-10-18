package org.kpc.rescaleframecraft.mixin;

import com.llamalad7.mixinextras.injector.wrapmethod.WrapMethod;
import com.llamalad7.mixinextras.injector.wrapoperation.Operation;
import com.mojang.blaze3d.vertex.BufferUploader;
import com.mojang.blaze3d.vertex.MeshData;
import org.kpc.rescaleframecraft.render.flywheel.ActorMotion;
import org.spongepowered.asm.mixin.Mixin;

@Mixin(BufferUploader.class)
abstract class ActorBufferUploaderMixin {
    @WrapMethod(method={"_drawWithShader","draw"})
    private static void rfc$draw(MeshData mesh,Operation<Void> original) {ActorMotion.scope(mesh,()->original.call(mesh));}
}
