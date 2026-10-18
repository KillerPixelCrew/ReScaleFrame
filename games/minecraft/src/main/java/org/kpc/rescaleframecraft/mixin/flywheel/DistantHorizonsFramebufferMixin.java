package org.kpc.rescaleframecraft.mixin.flywheel;

import net.irisshaders.iris.targets.Blaze3dRenderTargetExt;
import net.minecraft.client.Minecraft;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Pseudo;
import org.spongepowered.asm.mixin.Shadow;
import org.spongepowered.asm.mixin.Unique;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Pseudo
@Mixin(targets="net.irisshaders.iris.compat.dh.DHCompatInternal",remap=false)
abstract class DistantHorizonsFramebufferMixin {
    @Shadow private int cachedVersion;
    @Unique private int rfc$width,rfc$height;
    @Inject(method="createDepthTex",at=@At("TAIL"))
    private void rfc$allocatedSize(int width,int height,CallbackInfo ci) {rfc$width=width;rfc$height=height;}
    @Inject(method="reconnectDHTextures",at=@At("HEAD"))
    private void rfc$targetSize(int texture,CallbackInfo ci) {
        var target=Minecraft.getInstance().getMainRenderTarget();
        if(target.width!=rfc$width||target.height!=rfc$height)
            cachedVersion=((Blaze3dRenderTargetExt)target).iris$getDepthBufferVersion()^Integer.MIN_VALUE;
    }
}
