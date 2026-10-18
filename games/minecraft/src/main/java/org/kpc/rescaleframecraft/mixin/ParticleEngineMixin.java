package org.kpc.rescaleframecraft.mixin;

import com.mojang.blaze3d.systems.RenderSystem;
import com.mojang.blaze3d.vertex.VertexConsumer;
import net.minecraft.client.Camera;
import net.minecraft.client.Minecraft;
import net.minecraft.client.particle.Particle;
import net.minecraft.client.particle.ParticleEngine;
import net.minecraft.client.renderer.RenderType;
import net.minecraft.client.renderer.texture.TextureAtlas;
import org.kpc.rescaleframecraft.render.TemporalRenderer;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import com.llamalad7.mixinextras.injector.wrapoperation.Operation;
import com.llamalad7.mixinextras.injector.wrapoperation.WrapOperation;

@Mixin(ParticleEngine.class)
public abstract class ParticleEngineMixin {
    // An explicit descriptor also selects methods merged by another mixin.
    // A bare overloaded name excludes AsyncParticles' @Overwrite replacement.
    @WrapOperation(method="render(Lnet/minecraft/client/renderer/LightTexture;Lnet/minecraft/client/Camera;FLnet/minecraft/client/renderer/culling/Frustum;Ljava/util/function/Predicate;)V",at=@At(value="INVOKE",target="Lnet/minecraft/client/particle/Particle;render(Lcom/mojang/blaze3d/vertex/VertexConsumer;Lnet/minecraft/client/Camera;F)V"))
    private void rfc$particle(Particle particle,VertexConsumer consumer,Camera camera,float tick,Operation<Void> original) {
        if(!TemporalRenderer.capturing()) {original.call(particle,consumer,camera,tick);return;}
        int blocks=Minecraft.getInstance().getTextureManager().getTexture(TextureAtlas.LOCATION_BLOCKS).getId();
        var texture=RenderSystem.getShaderTexture(0)==blocks?TextureAtlas.LOCATION_BLOCKS:TextureAtlas.LOCATION_PARTICLES;
        var history=TemporalRenderer.GEOMETRY;
        history.push(particle);
        try { original.call(particle,history.wrap(consumer,RenderType.entityTranslucent(texture)),camera,tick); }
        finally { history.pop(); }
    }
}
