package org.kpc.rescaleframecraft.mixin.flywheel;

import java.util.concurrent.atomic.AtomicReference;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Pseudo;
import org.spongepowered.asm.mixin.gen.Accessor;

@Pseudo
@Mixin(targets={"dev.engine_room.flywheel.backend.engine.indirect.IndirectInstancer","dev.djefrey.colorwheel.indirect.ClrwlIndirectInstancer"},remap=false)
public interface IndirectInstancerAccessor {
    @Accessor("pages") AtomicReference<?> rfc$pages();
}
