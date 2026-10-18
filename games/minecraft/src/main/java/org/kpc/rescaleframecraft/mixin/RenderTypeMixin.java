package org.kpc.rescaleframecraft.mixin;

import com.llamalad7.mixinextras.injector.wrapmethod.WrapMethod;
import com.llamalad7.mixinextras.injector.wrapoperation.Operation;
import com.mojang.blaze3d.vertex.MeshData;
import net.minecraft.client.renderer.RenderType;
import org.kpc.rescaleframecraft.render.flywheel.HandMotion;
import org.spongepowered.asm.mixin.Mixin;

@Mixin(RenderType.class)
public abstract class RenderTypeMixin {
    @WrapMethod(method="draw")
    private void rfc$handMesh(MeshData mesh,Operation<Void> original) {
        HandMotion.draw((RenderType)(Object)this,mesh,()->original.call(mesh));
    }
}
