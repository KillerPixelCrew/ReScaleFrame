package org.kpc.rescaleframecraft.mixin.flywheel;

import dev.djefrey.colorwheel.instancing.ClrwlInstancedInstancer;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Pseudo;
import org.spongepowered.asm.mixin.gen.Accessor;

@Pseudo
@Mixin(targets="dev.djefrey.colorwheel.instancing.ClrwlInstancedDraw",remap=false)
public interface ColorwheelInstancedDrawAccessor {
    @Accessor("instancer") ClrwlInstancedInstancer<?> rfc$instancer();
    @Accessor("indexOfMeshInModel") int rfc$meshIndex();
}
