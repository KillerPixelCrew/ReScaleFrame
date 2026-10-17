package org.kpc.rescaleframecraft.mixin;

import com.llamalad7.mixinextras.injector.wrapoperation.Operation;
import com.llamalad7.mixinextras.injector.wrapoperation.WrapOperation;
import com.mojang.blaze3d.systems.RenderSystem;
import org.kpc.rescaleframecraft.render.NativeRenderer;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import com.llamalad7.mixinextras.injector.wrapmethod.WrapMethod;

@Mixin(RenderSystem.class)
public abstract class RenderSystemMixin {
    @WrapMethod(method="flipFrame")
    private static void rfc$displayScope(long window,Operation<Void> original) {
        NativeRenderer.beginDisplay();
        try {original.call(window);} finally {NativeRenderer.endDisplay();}
    }
    @WrapOperation(method="flipFrame",at=@At(value="INVOKE",target="Lorg/lwjgl/glfw/GLFW;glfwSwapBuffers(J)V",remap=false))
    private static void rfc$present(long window,Operation<Void> original) {if(!NativeRenderer.present())original.call(window);}
}
