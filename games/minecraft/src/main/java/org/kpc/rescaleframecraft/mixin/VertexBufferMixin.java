package org.kpc.rescaleframecraft.mixin;

import com.mojang.blaze3d.vertex.MeshData;
import com.mojang.blaze3d.vertex.VertexBuffer;
import com.llamalad7.mixinextras.injector.wrapoperation.Operation;
import com.llamalad7.mixinextras.injector.wrapoperation.WrapOperation;
import org.kpc.rescaleframecraft.render.flywheel.GpuMotion;
import org.kpc.rescaleframecraft.render.TemporalRenderer;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Mixin(VertexBuffer.class)
public abstract class VertexBufferMixin {
    @WrapOperation(method="draw",at=@At(value="INVOKE",target="Lcom/mojang/blaze3d/systems/RenderSystem;drawElements(III)V"))
    private void rfc$handDraw(int mode,int count,int type,Operation<Void> original) {
        GpuMotion.drawImmediate(()->original.call(mode,count,type));
    }
    @Inject(method="upload(Lcom/mojang/blaze3d/vertex/MeshData;)V",at=@At("HEAD"))
    private void rfc$meshReplaced(MeshData mesh,CallbackInfo ci) {
        TemporalRenderer.CLOUDS.meshChanged((VertexBuffer)(Object)this);
    }
}
