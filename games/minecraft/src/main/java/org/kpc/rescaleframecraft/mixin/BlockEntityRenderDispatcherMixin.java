package org.kpc.rescaleframecraft.mixin;

import com.mojang.blaze3d.vertex.PoseStack;
import net.minecraft.client.renderer.MultiBufferSource;
import net.minecraft.client.renderer.blockentity.BlockEntityRenderDispatcher;
import net.minecraft.world.level.block.entity.BlockEntity;
import org.kpc.rescaleframecraft.render.TemporalRenderer;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Mixin(BlockEntityRenderDispatcher.class)
public abstract class BlockEntityRenderDispatcherMixin {
    @Inject(method="render",at=@At("HEAD"))
    private void rfc$begin(BlockEntity entity,float tick,PoseStack pose,MultiBufferSource buffers,CallbackInfo ci) {
        TemporalRenderer.GEOMETRY.push(entity);
    }
    @Inject(method="render",at=@At("RETURN"))
    private void rfc$end(CallbackInfo ci) { TemporalRenderer.GEOMETRY.pop(); }
}
