package org.kpc.rescaleframecraft.mixin.flywheel;

import java.util.ArrayList;
import dev.engine_room.flywheel.api.instance.Instance;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Pseudo;
import org.spongepowered.asm.mixin.gen.Accessor;

@Pseudo
@Mixin(targets={"dev.engine_room.flywheel.backend.engine.BaseInstancer","dev.djefrey.colorwheel.engine.ClrwlBaseInstancer"},remap=false)
public interface BaseInstancerAccessor {
    @Accessor("instances") ArrayList<Instance> rfc$instances();
}
