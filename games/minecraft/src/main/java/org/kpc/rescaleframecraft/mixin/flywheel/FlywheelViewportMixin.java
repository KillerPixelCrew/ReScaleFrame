package org.kpc.rescaleframecraft.mixin.flywheel;

import com.llamalad7.mixinextras.injector.ModifyExpressionValue;
import net.minecraft.client.Minecraft;
import org.kpc.rescaleframecraft.render.TemporalRenderer;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Pseudo;
import org.spongepowered.asm.mixin.injection.At;

@Pseudo
@Mixin(targets="dev.engine_room.flywheel.backend.engine.uniform.FrameUniforms",remap=false)
abstract class FlywheelViewportMixin {
    @ModifyExpressionValue(method="update",at=@At(value="INVOKE",target="Lcom/mojang/blaze3d/platform/Window;getWidth()I",remap=true))
    private static int rfc$width(int width) {return TemporalRenderer.capturing()?Minecraft.getInstance().getMainRenderTarget().viewWidth:width;}
    @ModifyExpressionValue(method="update",at=@At(value="INVOKE",target="Lcom/mojang/blaze3d/platform/Window;getHeight()I",remap=true))
    private static int rfc$height(int height) {return TemporalRenderer.capturing()?Minecraft.getInstance().getMainRenderTarget().viewHeight:height;}
}
