package org.kpc.rescaleframecraft.mixin;

import com.mojang.blaze3d.vertex.PoseStack;
import net.minecraft.client.renderer.MultiBufferSource;
import net.minecraft.client.renderer.entity.EntityRenderDispatcher;
import net.minecraft.world.entity.Entity;
import org.kpc.rescaleframecraft.render.TemporalRenderer;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Mixin(EntityRenderDispatcher.class)
public abstract class EntityRenderDispatcherMixin {
    @Inject(method="render",at=@At("HEAD"))
    private void rfc$begin(Entity entity,double x,double y,double z,float yaw,float tick,PoseStack pose,MultiBufferSource buffers,int light,CallbackInfo ci) {
        TemporalRenderer.GEOMETRY.push(entity);
    }
    @Inject(method="render",at=@At("RETURN"))
    private void rfc$end(CallbackInfo ci) { TemporalRenderer.GEOMETRY.pop(); }
}
