package org.kpc.rescaleframecraft.mixin.flywheel;

import java.util.concurrent.atomic.AtomicInteger;
import dev.engine_room.flywheel.api.instance.Instance;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Pseudo;
import org.spongepowered.asm.mixin.gen.Accessor;

@Pseudo
@Mixin(targets={"dev.engine_room.flywheel.backend.engine.indirect.IndirectInstancer$InstancePage","dev.djefrey.colorwheel.indirect.ClrwlIndirectInstancer$InstancePage"},remap=false)
public interface InstancePageAccessor {
    @Accessor("instances") Instance[] rfc$instances();
    @Accessor("valid") AtomicInteger rfc$valid();
}
