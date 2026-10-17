package org.kpc.rescaleframecraft.mixin;

import com.mojang.blaze3d.systems.RenderSystem;
import com.mojang.blaze3d.vertex.VertexBuffer;
import com.llamalad7.mixinextras.injector.wrapoperation.Operation;
import com.llamalad7.mixinextras.injector.wrapoperation.WrapOperation;
import net.minecraft.client.renderer.ShaderInstance;
import org.joml.Matrix4f;
import org.kpc.rescaleframecraft.render.TemporalRenderer;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Unique;
import org.spongepowered.asm.mixin.injection.At;
import org.kpc.rescaleframecraft.ReScaleFrameCraft;
import org.lwjgl.opengl.GL11;

/** Sodium owns a separate position/colour cloud mesh and bypasses vanilla's draw. */
@Mixin(targets="net.caffeinemc.mods.sodium.client.render.immediate.CloudRenderer",remap=false)
public abstract class SodiumCloudRendererMixin {
    @Unique private boolean rfc$logged;
    @WrapOperation(method="render(Lnet/minecraft/client/Camera;Lnet/minecraft/client/multiplayer/ClientLevel;Lorg/joml/Matrix4f;Lcom/mojang/blaze3d/vertex/PoseStack;FF)V",at=@At(value="INVOKE",target="Lcom/mojang/blaze3d/vertex/VertexBuffer;drawWithShader(Lorg/joml/Matrix4f;Lorg/joml/Matrix4f;Lnet/minecraft/client/renderer/ShaderInstance;)V"))
    private void rfc$cloud(VertexBuffer mesh,Matrix4f view,Matrix4f projection,ShaderInstance shader,Operation<Void> original) {
        if(TemporalRenderer.capturing()) {
            TemporalRenderer.CLOUDS.capture(mesh,view,projection,0,RenderSystem.getShaderColor()[3],TemporalRenderer.CAMERA);
            if(!rfc$logged) {
                ReScaleFrameCraft.LOGGER.info("Capturing Sodium cloud motion: format={}, nativeDepthWrite={}, alpha={}",mesh.getFormat(),GL11.glGetBoolean(GL11.GL_DEPTH_WRITEMASK),RenderSystem.getShaderColor()[3]);
                rfc$logged=true;
            }
        }
        original.call(mesh,view,projection,shader);
    }
}
