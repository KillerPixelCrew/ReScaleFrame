package org.kpc.rescaleframecraft.mixin;

import com.mojang.blaze3d.vertex.MeshData;
import com.mojang.blaze3d.vertex.VertexBuffer;
import org.kpc.rescaleframecraft.render.TemporalRenderer;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Mixin(VertexBuffer.class)
public abstract class VertexBufferMixin {
    @Inject(method="upload(Lcom/mojang/blaze3d/vertex/MeshData;)V",at=@At("HEAD"))
    private void rfc$meshReplaced(MeshData mesh,CallbackInfo ci) {
        TemporalRenderer.CLOUDS.meshChanged((VertexBuffer)(Object)this);
    }
}
