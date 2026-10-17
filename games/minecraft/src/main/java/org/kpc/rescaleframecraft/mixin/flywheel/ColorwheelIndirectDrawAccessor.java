package org.kpc.rescaleframecraft.mixin.flywheel;

import dev.djefrey.colorwheel.indirect.ClrwlIndirectInstancer;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Pseudo;
import org.spongepowered.asm.mixin.gen.Accessor;

@Pseudo
@Mixin(targets="dev.djefrey.colorwheel.indirect.ClrwlIndirectDraw",remap=false)
public interface ColorwheelIndirectDrawAccessor {
    @Accessor("instancer") ClrwlIndirectInstancer<?> rfc$instancer();
}
