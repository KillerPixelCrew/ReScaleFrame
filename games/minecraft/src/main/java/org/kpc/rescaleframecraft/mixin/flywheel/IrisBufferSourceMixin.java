package org.kpc.rescaleframecraft.mixin.flywheel;

import com.mojang.blaze3d.vertex.VertexConsumer;
import net.irisshaders.batchedentityrendering.impl.WrappableRenderType;
import net.minecraft.client.renderer.RenderType;
import org.kpc.rescaleframecraft.render.TemporalRenderer;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Pseudo;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

@Pseudo
@Mixin(targets="net.irisshaders.batchedentityrendering.impl.FullyBufferedMultiBufferSource",remap=false)
abstract class IrisBufferSourceMixin {
    @Inject(method="getBuffer",at=@At("RETURN"),cancellable=true)
    private void rfc$capture(RenderType type,CallbackInfoReturnable<VertexConsumer> cir) {
        if(!TemporalRenderer.capturing())return;
        // Iris recreates wrapper types for shader IDs. The underlying material
        // is the stable key; the actor identity is tracked separately.
        while(type instanceof WrappableRenderType wrapped)type=wrapped.unwrap();
        cir.setReturnValue(TemporalRenderer.GEOMETRY.wrap(cir.getReturnValue(),type));
    }
}
