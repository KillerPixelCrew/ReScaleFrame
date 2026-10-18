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
import com.llamalad7.mixinextras.injector.wrapmethod.WrapMethod;
import com.llamalad7.mixinextras.injector.wrapoperation.Operation;

@Mixin(BlockEntityRenderDispatcher.class)
public abstract class BlockEntityRenderDispatcherMixin {
    @WrapMethod(method="render")
    private void rfc$actor(BlockEntity entity,float tick,PoseStack pose,MultiBufferSource buffers,Operation<Void> original) {
        boolean capture=TemporalRenderer.capturing();
        if(capture)TemporalRenderer.GEOMETRY.push(entity);
        try {original.call(entity,tick,pose,buffers);}
        finally {if(capture)TemporalRenderer.GEOMETRY.pop();}
    }
}
