package org.kpc.rescaleframecraft.mixin.flywheel;

import java.util.List;
import dev.djefrey.colorwheel.indirect.ClrwlIndirectBuffers;
import dev.djefrey.colorwheel.indirect.ClrwlIndirectInstancer;
import dev.djefrey.colorwheel.indirect.ClrwlIndirectDraw;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Pseudo;
import org.spongepowered.asm.mixin.gen.Accessor;

@Pseudo
@Mixin(targets="dev.djefrey.colorwheel.indirect.ClrwlIndirectCullingGroup",remap=false)
public interface ColorwheelGroupAccessor {
    @Accessor("instanceStride") long rfc$stride();
    @Accessor("buffers") ClrwlIndirectBuffers rfc$buffers();
    @Accessor("instancers") List<ClrwlIndirectInstancer<?>> rfc$instancers();
    @Accessor("indirectDraws") List<ClrwlIndirectDraw> rfc$draws();
}
