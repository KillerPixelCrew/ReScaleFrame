package org.kpc.rescaleframecraft.mixin;

import net.minecraft.client.KeyboardHandler;
import org.kpc.rescaleframecraft.render.NativeRenderer;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Mixin(KeyboardHandler.class)
abstract class KeyboardHandlerMixin {
    @Inject(method="keyPress",at=@At("HEAD"))
    private void rfc$sample(long window,int key,int scanCode,int action,int modifiers,CallbackInfo ci) {
        NativeRenderer.input(1);
    }
}
