package org.kpc.rescaleframecraft.mixin.flywheel;

import dev.engine_room.flywheel.backend.engine.instancing.InstancedInstancer;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Pseudo;
import org.spongepowered.asm.mixin.gen.Accessor;

@Pseudo
@Mixin(targets="dev.engine_room.flywheel.backend.engine.instancing.InstancedDraw",remap=false)
public interface InstancedDrawAccessor {
    @Accessor("instancer") InstancedInstancer<?> rfc$instancer();
    @Accessor("indexOfMeshInModel") int rfc$meshIndex();
}
