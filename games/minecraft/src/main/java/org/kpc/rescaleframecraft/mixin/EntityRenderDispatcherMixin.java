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
import com.llamalad7.mixinextras.injector.wrapmethod.WrapMethod;
import com.llamalad7.mixinextras.injector.wrapoperation.Operation;

@Mixin(EntityRenderDispatcher.class)
public abstract class EntityRenderDispatcherMixin {
    @WrapMethod(method="render")
    private void rfc$actor(Entity entity,double x,double y,double z,float yaw,float tick,PoseStack pose,MultiBufferSource buffers,int light,Operation<Void> original) {
        boolean capture=TemporalRenderer.capturing();
        if(capture)TemporalRenderer.GEOMETRY.push(entity);
        try {original.call(entity,x,y,z,yaw,tick,pose,buffers,light);}
        finally {if(capture)TemporalRenderer.GEOMETRY.pop();}
    }
}
