package org.kpc.rescaleframecraft.mixin;

import com.mojang.blaze3d.vertex.VertexConsumer;
import net.minecraft.client.renderer.MultiBufferSource;
import net.minecraft.client.renderer.RenderType;
import org.kpc.rescaleframecraft.render.TemporalRenderer;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

@Mixin(MultiBufferSource.BufferSource.class)
public abstract class BufferSourceMixin {
    @Inject(method="getBuffer",at=@At("RETURN"),cancellable=true)
    private void rfc$capture(RenderType type,CallbackInfoReturnable<VertexConsumer> cir) {
        if(!TemporalRenderer.capturing())return;
        cir.setReturnValue(TemporalRenderer.GEOMETRY.wrap(cir.getReturnValue(),type));
    }
}
