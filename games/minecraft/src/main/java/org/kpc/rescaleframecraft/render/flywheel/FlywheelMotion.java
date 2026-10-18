package org.kpc.rescaleframecraft.render.flywheel;

import dev.engine_room.flywheel.api.model.Model;
import org.kpc.rescaleframecraft.mixin.flywheel.*;
import org.lwjgl.BufferUtils;

/** Stable instance/model adapters; GPU replay itself has no Flywheel dependency. */
public final class FlywheelMotion {
    private FlywheelMotion() {}
    public static void draw(Runnable original) {GpuMotion.draw(original);}
    public static void forget(int source) {GpuMotion.forget(source);}
    public static void indirect(Object owner) {
        GpuMotion.select(null,0,-1);if(!GpuMotion.capturing())return;
        var access=(IndirectGroupAccessor)owner;
        var group=GpuMotion.group(owner);
        if(group.begin(Math.toIntExact(access.rfc$buffers().objectStorage.objectBuffer.capacity()/access.rfc$stride()))) {
            for(var instancer:access.rfc$instancers()) {
                Object[] pages=(Object[])((IndirectInstancerAccessor)instancer).rfc$pages().get();
                Model model=instancer.recreate.key().model();
                for(int page=0;page<pages.length;page++) {
                    var contents=(InstancePageAccessor)pages[page];int valid=contents.rfc$valid().get();
                    while(valid!=0) {
                        int slot=Integer.numberOfTrailingZeros(valid);valid&=valid-1;
                        group.record(contents.rfc$instances()[slot],model,instancer.local2GlobalInstanceIndex(page*32+slot),vertices(model));
                    }
                }
            }
            var list=access.rfc$draws();
            if(group.meshData.capacity()<list.size())group.meshData=BufferUtils.createIntBuffer(list.size());
            group.meshData.clear().limit(Math.max(1,list.size()));
            for(int i=0;i<list.size();i++) {
                var draw=list.get(i);var instancer=((IndirectDrawAccessor)draw).rfc$instancer();
                group.meshData.put(i,prefix(instancer.recreate.key().model(),draw.indexOfMeshInModel()));
            }
            group.meshes.upload(group.meshData);group.ready();
        }
        GpuMotion.select(group,0,-1);
    }
    public static void instanced(Object owner) {
        GpuMotion.select(null,0,-1);if(!GpuMotion.capturing())return;
        var draw=(InstancedDrawAccessor)owner;var instancer=draw.rfc$instancer();
        var group=GpuMotion.group(instancer);
        var instances=((BaseInstancerAccessor)instancer).rfc$instances();Model model=instancer.recreate.key().model();
        if(group.begin(instances.size())) {
            for(int i=0;i<instances.size();i++)group.record(instances.get(i),model,i,vertices(model));
            group.meshData.clear().limit(1);group.meshData.put(0,0);group.meshes.upload(group.meshData);group.ready();
        }
        GpuMotion.select(group,prefix(model,draw.rfc$meshIndex()),-1);
    }
    public static void colorwheelIndirect(Object owner) {
        GpuMotion.select(null,0,-1);if(!GpuMotion.capturing())return;
        var access=(ColorwheelGroupAccessor)owner;
        var group=GpuMotion.group(owner);
        if(group.begin(Math.toIntExact(access.rfc$buffers().objectStorage.objectBuffer.capacity()/access.rfc$stride()))) {
            for(var instancer:access.rfc$instancers()) {
                Object[] pages=(Object[])((IndirectInstancerAccessor)instancer).rfc$pages().get();
                Model model=instancer.recreate.key().model();
                for(int page=0;page<pages.length;page++) {
                    var contents=(InstancePageAccessor)pages[page];int valid=contents.rfc$valid().get();
                    while(valid!=0) {
                        int slot=Integer.numberOfTrailingZeros(valid);valid&=valid-1;
                        group.record(contents.rfc$instances()[slot],model,instancer.local2GlobalInstanceIndex(page*32+slot),vertices(model));
                    }
                }
            }
            var list=access.rfc$draws();
            if(group.meshData.capacity()<list.size())group.meshData=BufferUtils.createIntBuffer(list.size());
            group.meshData.clear().limit(Math.max(1,list.size()));
            for(int i=0;i<list.size();i++) {
                var draw=list.get(i);var instancer=((ColorwheelIndirectDrawAccessor)draw).rfc$instancer();
                group.meshData.put(i,prefix(instancer.recreate.key().model(),draw.indexOfMeshInModel()));
            }
            group.meshes.upload(group.meshData);group.ready();
        }
        GpuMotion.select(group,0,-1);
    }
    public static void colorwheelInstanced(Object owner) {
        GpuMotion.select(null,0,-1);if(!GpuMotion.capturing())return;
        var draw=(ColorwheelInstancedDrawAccessor)owner;var instancer=draw.rfc$instancer();
        var group=GpuMotion.group(instancer);
        var instances=((BaseInstancerAccessor)instancer).rfc$instances();Model model=instancer.recreate.key().model();
        if(group.begin(instances.size())) {
            for(int i=0;i<instances.size();i++)group.record(instances.get(i),model,i,vertices(model));
            group.meshData.clear().limit(1);group.meshData.put(0,0);group.meshes.upload(group.meshData);group.ready();
        }
        GpuMotion.select(group,prefix(model,draw.rfc$meshIndex()),-1);
    }
    public static void crumbling(Object owner) {
        if(!GpuMotion.capturing()||GpuMotion.selected()==null)return;
        if(owner instanceof ColorwheelIndirectDrawAccessor access) {
            var draw=(dev.djefrey.colorwheel.indirect.ClrwlIndirectDraw)owner;
            GpuMotion.overrideMesh(prefix(access.rfc$instancer().recreate.key().model(),draw.indexOfMeshInModel()));
        } else {
            var draw=(dev.engine_room.flywheel.backend.engine.indirect.IndirectDraw)owner;
            GpuMotion.overrideMesh(prefix(((IndirectDrawAccessor)owner).rfc$instancer().recreate.key().model(),draw.indexOfMeshInModel()));
        }
    }
    private static int vertices(Model model) {int sum=0;for(var mesh:model.meshes())sum=Math.addExact(sum,mesh.mesh().vertexCount());return sum;}
    private static int prefix(Model model,int meshIndex) {int sum=0;for(int i=0;i<meshIndex;i++)sum+=model.meshes().get(i).mesh().vertexCount();return sum;}
}
