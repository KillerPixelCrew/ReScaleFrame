package org.kpc.rescaleframecraft.render.flywheel;

import net.caffeinemc.mods.sodium.client.gl.arena.GlBufferSegment;
import net.caffeinemc.mods.sodium.client.render.chunk.data.SectionRenderDataStorage;
import net.caffeinemc.mods.sodium.client.render.chunk.lists.ChunkRenderList;
import net.caffeinemc.mods.sodium.client.render.chunk.region.RenderRegion;
import org.lwjgl.BufferUtils;

/** Matches section-local vertices across Sodium arena moves, culling and translucent index sorting. */
public final class TerrainMotion {
    private TerrainMotion() {}
    public interface Meshes {
        GlBufferSegment[] rfc$allocations();
        Object rfc$generation(int section);
    }
    public static void render(SectionRenderDataStorage storage,ChunkRenderList list,Runnable original) {
        if(!GpuMotion.capturing()) {original.run();return;}
        var group=GpuMotion.group(storage);
        if(group.begin(RenderRegion.REGION_SIZE)) {
            var meshes=(Meshes)storage;
            if(group.meshData.capacity()<RenderRegion.REGION_SIZE)group.meshData=BufferUtils.createIntBuffer(RenderRegion.REGION_SIZE);
            group.meshData.clear().limit(RenderRegion.REGION_SIZE);
            for(int i=0;i<RenderRegion.REGION_SIZE;i++)group.meshData.put(i,0);
            var sections=list.sectionsWithGeometryIterator(false);
            if(sections!=null)while(sections.hasNext()) {
                int section=sections.nextByteAsInt();var allocation=meshes.rfc$allocations()[section];
                if(allocation==null)continue;
                Object generation=meshes.rfc$generation(section);
                group.record(generation,generation,section,Math.toIntExact(allocation.getLength()));
                group.meshData.put(section,Math.toIntExact(allocation.getOffset()));
            }
            group.meshes.upload(group.meshData);group.ready();
        }
        GpuMotion.scoped(group,original);
    }
}
