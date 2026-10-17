package org.kpc.rescaleframecraft.mixin;

import net.minecraft.client.*;
import net.minecraft.client.renderer.*;
import org.joml.Matrix4f;
import org.kpc.rescaleframecraft.render.TemporalRenderer;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.Redirect;
import com.mojang.blaze3d.systems.RenderSystem;
import com.mojang.blaze3d.vertex.VertexBuffer;
import com.mojang.blaze3d.vertex.MeshData;
import com.mojang.blaze3d.vertex.BufferUploader;
import net.minecraft.resources.ResourceLocation;
import org.spongepowered.asm.mixin.Unique;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Mixin(value=LevelRenderer.class,priority=1100)
public abstract class LevelRendererMixin {
    @Unique private final Object rfc$weather=new Object();
    @Inject(method="renderLevel",at=@At("HEAD"))
    private void rfc$begin(DeltaTracker delta,boolean outline,Camera camera,GameRenderer renderer,LightTexture light,
                           Matrix4f view,Matrix4f projection,CallbackInfo ci) {
        TemporalRenderer.begin(camera,view,projection);
    }
    @Redirect(method="renderClouds",at=@At(value="INVOKE",target="Lcom/mojang/blaze3d/vertex/VertexBuffer;drawWithShader(Lorg/joml/Matrix4f;Lorg/joml/Matrix4f;Lnet/minecraft/client/renderer/ShaderInstance;)V"))
    private void rfc$cloud(VertexBuffer mesh,Matrix4f view,Matrix4f projection,ShaderInstance shader) {
        if(TemporalRenderer.capturing())TemporalRenderer.CLOUDS.capture(mesh,view,projection,RenderSystem.getShaderTexture(0),TemporalRenderer.CAMERA);
        mesh.drawWithShader(view,projection,shader);
    }
    @Redirect(method="renderSnowAndRain",at=@At(value="INVOKE",target="Lcom/mojang/blaze3d/vertex/BufferUploader;drawWithShader(Lcom/mojang/blaze3d/vertex/MeshData;)V"))
    private void rfc$weather(MeshData mesh) {
        if(TemporalRenderer.capturing()) {
            var textures=Minecraft.getInstance().getTextureManager();
            var rain=ResourceLocation.withDefaultNamespace("textures/environment/rain.png");
            var snow=ResourceLocation.withDefaultNamespace("textures/environment/snow.png");
            var location=RenderSystem.getShaderTexture(0)==textures.getTexture(rain).getId()?rain:snow;
            TemporalRenderer.GEOMETRY.capture(rfc$weather,RenderType.entityTranslucent(location),mesh);
        }
        BufferUploader.drawWithShader(mesh);
    }
}
