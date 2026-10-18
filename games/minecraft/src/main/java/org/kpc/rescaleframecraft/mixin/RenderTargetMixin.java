package org.kpc.rescaleframecraft.mixin;

import com.mojang.blaze3d.pipeline.RenderTarget;
import org.kpc.rescaleframecraft.render.WorldRenderTarget;
import org.lwjgl.opengl.GL30;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.ModifyArg;

@Mixin(RenderTarget.class)
abstract class RenderTargetMixin {
    @ModifyArg(method="createBuffers",at=@At(value="INVOKE",target="Lcom/mojang/blaze3d/platform/GlStateManager;_texImage2D(IIIIIIIILjava/nio/IntBuffer;)V"),index=2)
    private int rfc$sizedWorldDepth(int format) {
        // Unsized DEPTH_COMPONENT plus FLOAT and Iris's UNSIGNED_SHORT copy
        // allocation can select different driver formats. Declare the scene's
        // storage at its original allocation, including Iris's texture metadata.
        return (Object)this instanceof WorldRenderTarget&&format==GL30.GL_DEPTH_COMPONENT?GL30.GL_DEPTH_COMPONENT32F:format;
    }
}
