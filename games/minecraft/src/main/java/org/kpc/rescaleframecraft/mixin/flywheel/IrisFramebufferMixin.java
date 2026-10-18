package org.kpc.rescaleframecraft.mixin.flywheel;

import net.irisshaders.iris.gl.IrisRenderSystem;
import net.irisshaders.iris.gl.framebuffer.GlFramebuffer;
import org.lwjgl.opengl.GL30;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Pseudo;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Pseudo
@Mixin(targets="net.irisshaders.iris.gl.framebuffer.GlFramebuffer",remap=false)
abstract class IrisFramebufferMixin {
    @Inject(method="addDepthAttachment",at=@At("HEAD"))
    private void rfc$replaceDepthStencil(int texture,CallbackInfo ci) {
        // A preceding combined attachment must not leave its stencil image bound
        // when Iris replaces it with a depth-only world texture.
        IrisRenderSystem.framebufferTexture2D(((GlFramebuffer)(Object)this).getId(),GL30.GL_FRAMEBUFFER,
            GL30.GL_STENCIL_ATTACHMENT,GL30.GL_TEXTURE_2D,0,0);
    }
}
