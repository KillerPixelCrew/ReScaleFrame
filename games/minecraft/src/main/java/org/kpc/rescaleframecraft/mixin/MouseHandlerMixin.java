package org.kpc.rescaleframecraft.mixin;

import net.minecraft.client.MouseHandler;
import org.kpc.rescaleframecraft.render.NativeRenderer;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Mixin(MouseHandler.class)
abstract class MouseHandlerMixin {
    @Inject(method={"onMove","onPress","onScroll"},at=@At("HEAD"))
    private void rfc$sample(CallbackInfo ci) {NativeRenderer.input(2);}
}
