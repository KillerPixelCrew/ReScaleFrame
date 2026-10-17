package org.kpc.rescaleframecraft.mixin.flywheel;

import java.util.List;
import dev.engine_room.flywheel.backend.engine.indirect.IndirectBuffers;
import dev.engine_room.flywheel.backend.engine.indirect.IndirectInstancer;
import dev.engine_room.flywheel.backend.engine.indirect.IndirectDraw;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Pseudo;
import org.spongepowered.asm.mixin.gen.Accessor;

@Pseudo
@Mixin(targets="dev.engine_room.flywheel.backend.engine.indirect.IndirectCullingGroup",remap=false)
public interface IndirectGroupAccessor {
    @Accessor("instanceStride") long rfc$stride();
    @Accessor("buffers") IndirectBuffers rfc$buffers();
    @Accessor("instancers") List<IndirectInstancer<?>> rfc$instancers();
    @Accessor("indirectDraws") List<IndirectDraw> rfc$draws();
}
