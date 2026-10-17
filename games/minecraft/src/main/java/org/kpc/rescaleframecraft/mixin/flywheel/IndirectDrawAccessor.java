package org.kpc.rescaleframecraft.mixin.flywheel;

import dev.engine_room.flywheel.backend.engine.indirect.IndirectInstancer;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Pseudo;
import org.spongepowered.asm.mixin.gen.Accessor;

@Pseudo
@Mixin(targets="dev.engine_room.flywheel.backend.engine.indirect.IndirectDraw",remap=false)
public interface IndirectDrawAccessor {
    @Accessor("instancer") IndirectInstancer<?> rfc$instancer();
}
