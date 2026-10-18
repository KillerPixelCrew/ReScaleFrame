package org.kpc.rescaleframecraft.mixin.flywheel;

import net.irisshaders.iris.gl.texture.DepthBufferFormat;
import org.kpc.rescaleframecraft.ReScaleFrameCraft;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Pseudo;
import org.spongepowered.asm.mixin.Shadow;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

@Pseudo
@Mixin(targets="net.irisshaders.iris.targets.RenderTargets",remap=false)
abstract class IrisRenderTargetsMixin {
    @Shadow private int currentDepthTexture;
    @Shadow private DepthBufferFormat currentDepthFormat;
    @Shadow private int cachedDepthBufferVersion;

    @Inject(method="resizeIfNeeded",at=@At("HEAD"))
    private void rfc$depthIdentity(int version,int texture,int width,int height,DepthBufferFormat format,
        net.irisshaders.iris.shaderpack.properties.PackDirectives directives,CallbackInfoReturnable<Boolean> cir) {
        // Iris's version is local to one RenderTarget. Display/world targets can
        // have equal versions while owning different textures and depth formats.
        if(currentDepthTexture!=texture||currentDepthFormat!=format) {
            ReScaleFrameCraft.LOGGER.info("Iris depth target changed: texture {} -> {}, format {} -> {}, {}x{}",
                currentDepthTexture,texture,currentDepthFormat,format,width,height);
            cachedDepthBufferVersion=version^Integer.MIN_VALUE;
        }
    }
}
