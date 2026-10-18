package org.kpc.rescaleframecraft.mixin.flywheel;

import net.caffeinemc.mods.sodium.client.gl.arena.GlBufferSegment;
import net.caffeinemc.mods.sodium.client.render.chunk.data.SectionRenderDataStorage;
import net.caffeinemc.mods.sodium.client.render.chunk.region.RenderRegion;
import org.kpc.rescaleframecraft.render.flywheel.TerrainMotion;
import org.spongepowered.asm.mixin.Final;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Shadow;
import org.spongepowered.asm.mixin.Unique;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Mixin(value=SectionRenderDataStorage.class,remap=false)
abstract class SodiumSectionStorageMixin implements TerrainMotion.Meshes {
    @Shadow @Final private GlBufferSegment[] vertexAllocations;
    @Unique private final Object[] rfc$generations=new Object[RenderRegion.REGION_SIZE];
    @Inject(method="setVertexData",at=@At("HEAD"))
    private void rfc$meshRebuilt(int section,GlBufferSegment allocation,int[] segments,CallbackInfo ci) {
        rfc$generations[section]=new Object();
    }
    @Override public GlBufferSegment[] rfc$allocations() {return vertexAllocations;}
    @Override public Object rfc$generation(int section) {return rfc$generations[section];}
}
